use std::collections::HashMap;
use std::io;
use std::os::fd::{AsFd, AsRawFd};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use cosmic_protocols::output_management::v1::client::{
    zcosmic_output_head_v1 as cosmic_head, zcosmic_output_manager_v1 as cosmic_manager,
};
use gio::glib;
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
    backend::ObjectId,
    delegate_noop, event_created_child,
    globals::{self, GlobalListContents},
    protocol::wl_registry,
};
use wayland_protocols_wlr::output_management::v1::client::{
    zwlr_output_head_v1 as output_head, zwlr_output_manager_v1 as output_manager,
    zwlr_output_mode_v1 as output_mode,
};

#[derive(Default)]
struct Head {
    name: String,
    enabled: bool,
    primary: bool,
    mirrored: bool,
    position: (i32, i32),
    extension: Option<cosmic_head::ZcosmicOutputHeadV1>,
}

fn primary<'a>(heads: impl Iterator<Item = &'a Head>) -> Option<&'a str> {
    heads
        .filter(|head| head.enabled && !head.mirrored && !head.name.is_empty())
        .min_by_key(|head| (!head.primary, head.position != (0, 0), head.name.as_str()))
        .map(|head| head.name.as_str())
}

#[derive(Default)]
struct OutputState {
    heads: HashMap<ObjectId, Head>,
    cosmic: Option<cosmic_manager::ZcosmicOutputManagerV1>,
    primary: Option<String>,
}

pub(super) struct Outputs {
    events: EventQueue<OutputState>,
    state: OutputState,
    source: glib::Source,
    ready: Arc<AtomicBool>,
}

impl Outputs {
    pub(super) fn open(context: &glib::MainContext) -> io::Result<Self> {
        let connection = Connection::connect_to_env().map_err(io::Error::other)?;
        let (globals, mut events) =
            globals::registry_queue_init(&connection).map_err(io::Error::other)?;
        let queue = events.handle();
        let mut state = OutputState {
            cosmic: globals.bind(&queue, 1..=3, ()).ok(),
            ..OutputState::default()
        };
        let _manager = globals
            .bind::<output_manager::ZwlrOutputManagerV1, _, _>(&queue, 1..=4, ())
            .map_err(io::Error::other)?;
        events.roundtrip(&mut state).map_err(io::Error::other)?;
        events.roundtrip(&mut state).map_err(io::Error::other)?;

        let ready = Arc::new(AtomicBool::new(false));
        let wakeup = ready.clone();
        let source = glib::source::unix_fd_source_new(
            connection.as_fd().as_raw_fd(),
            glib::IOCondition::IN | glib::IOCondition::ERR | glib::IOCondition::HUP,
            Some("cosmic-output-changes"),
            glib::Priority::DEFAULT,
            move |_, _| {
                wakeup.store(true, Ordering::Relaxed);
                glib::ControlFlow::Continue
            },
        );
        source.attach(Some(context));
        Ok(Self {
            events,
            state,
            source,
            ready,
        })
    }

    pub(super) fn primary(&self) -> Option<&str> {
        self.state.primary.as_deref()
    }

    pub(super) fn refresh(&mut self) -> io::Result<bool> {
        if !self.ready.swap(false, Ordering::Relaxed) {
            return Ok(false);
        }
        let previous = self.primary().map(str::to_owned);
        if let Some(read) = self.events.prepare_read() {
            match read.read() {
                Ok(_) => {}
                Err(wayland_client::backend::WaylandError::Io(error))
                    if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(io::Error::other(error)),
            }
        }
        self.events
            .dispatch_pending(&mut self.state)
            .map_err(io::Error::other)?;
        self.events.flush().map_err(io::Error::other)?;
        Ok(self.primary() != previous.as_deref())
    }
}

impl Drop for Outputs {
    fn drop(&mut self) {
        self.source.destroy();
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for OutputState {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<output_mode::ZwlrOutputModeV1, ()> for OutputState {
    fn event(
        _: &mut Self,
        mode: &output_mode::ZwlrOutputModeV1,
        event: output_mode::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if matches!(event, output_mode::Event::Finished) && mode.version() >= 3 {
            mode.release();
        }
    }
}

impl Dispatch<output_manager::ZwlrOutputManagerV1, ()> for OutputState {
    event_created_child!(OutputState, output_manager::ZwlrOutputManagerV1, [
        output_manager::EVT_HEAD_OPCODE => (output_head::ZwlrOutputHeadV1, ())
    ]);

    fn event(
        state: &mut Self,
        _: &output_manager::ZwlrOutputManagerV1,
        event: output_manager::Event,
        _: &(),
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        match event {
            output_manager::Event::Head { head } => {
                let id = head.id();
                let extension = state
                    .cosmic
                    .as_ref()
                    .map(|manager| manager.get_head(&head, queue, id.clone()));
                state.heads.insert(
                    id,
                    Head {
                        extension,
                        ..Head::default()
                    },
                );
            }
            output_manager::Event::Done { .. } => {
                state.primary = primary(state.heads.values()).map(str::to_owned);
            }
            output_manager::Event::Finished => state.primary = None,
            _ => {}
        }
    }
}

impl Dispatch<output_head::ZwlrOutputHeadV1, ()> for OutputState {
    event_created_child!(OutputState, output_head::ZwlrOutputHeadV1, [
        output_head::EVT_MODE_OPCODE => (output_mode::ZwlrOutputModeV1, ())
    ]);

    fn event(
        state: &mut Self,
        proxy: &output_head::ZwlrOutputHeadV1,
        event: output_head::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let output_head::Event::Finished = event {
            if let Some(extension) = state
                .heads
                .remove(&proxy.id())
                .and_then(|head| head.extension)
            {
                extension.release();
            }
            if proxy.version() >= 3 {
                proxy.release();
            }
            return;
        }
        let Some(head) = state.heads.get_mut(&proxy.id()) else {
            return;
        };
        match event {
            output_head::Event::Name { name } => head.name = name,
            output_head::Event::Enabled { enabled } => {
                head.enabled = enabled != 0;
                if !head.enabled {
                    head.primary = false;
                    head.mirrored = false;
                }
            }
            output_head::Event::Position { x, y } => head.position = (x, y),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chooses_active_primary_then_layout_origin() {
        let mut laptop = Head {
            name: "eDP-1".into(),
            enabled: true,
            primary: true,
            position: (1920, 0),
            ..Head::default()
        };
        let external = Head {
            name: "HDMI-A-1".into(),
            enabled: true,
            ..Head::default()
        };
        assert_eq!(primary([&laptop, &external].into_iter()), Some("eDP-1"));
        laptop.enabled = false;
        assert_eq!(primary([&laptop, &external].into_iter()), Some("HDMI-A-1"));
        laptop.enabled = true;
        laptop.primary = false;
        assert_eq!(primary([&laptop, &external].into_iter()), Some("HDMI-A-1"));
    }
}

delegate_noop!(OutputState: ignore cosmic_manager::ZcosmicOutputManagerV1);

impl Dispatch<cosmic_head::ZcosmicOutputHeadV1, ObjectId> for OutputState {
    fn event(
        state: &mut Self,
        _: &cosmic_head::ZcosmicOutputHeadV1,
        event: cosmic_head::Event,
        id: &ObjectId,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(head) = state.heads.get_mut(id) else {
            return;
        };
        match event {
            cosmic_head::Event::XwaylandPrimary { state } => head.primary = state != 0,
            cosmic_head::Event::Mirroring { name } => head.mirrored = name.is_some(),
            _ => {}
        }
    }
}
