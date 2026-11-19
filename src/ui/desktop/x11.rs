use std::ffi::{CStr, c_void};
use std::io;
use std::os::raw::{c_char, c_int, c_uchar, c_uint, c_ulong};
use std::ptr::{self, NonNull};

use super::Focus;

#[repr(C)]
struct Cookie {
    sequence: c_uint,
}

#[repr(C)]
struct ScreenIterator {
    data: *const u32,
    remaining: c_int,
    index: c_int,
}

#[repr(C)]
struct AtomReply {
    headers: [u8; 8],
    atom: u32,
}

#[repr(C)]
struct PropertyReply {
    response_type: u8,
    format: u8,
    sequence: u16,
    length: u32,
    kind: u32,
    bytes_after: u32,
    value_len: u32,
}

fn property_size(reply: &PropertyReply, format: u8) -> Option<usize> {
    if reply.format != format || reply.bytes_after != 0 {
        return None;
    }
    let values = usize::try_from(reply.value_len).ok()?;
    let bytes = values.checked_mul(usize::from(format / 8))?;
    let payload = usize::try_from(reply.length).ok()?.checked_mul(4)?;
    (bytes != 0 && bytes <= 1024 && bytes <= payload).then_some(bytes)
}

#[link(name = "xcb")]
unsafe extern "C" {
    fn xcb_connect(name: *const c_char, screen: *mut c_int) -> *mut c_void;
    fn xcb_connection_has_error(connection: *mut c_void) -> c_int;
    fn xcb_disconnect(connection: *mut c_void);
    fn xcb_get_setup(connection: *mut c_void) -> *const c_void;
    fn xcb_setup_roots_iterator(setup: *const c_void) -> ScreenIterator;
    fn xcb_screen_next(iterator: *mut ScreenIterator);
    fn xcb_intern_atom(
        connection: *mut c_void,
        only_if_exists: u8,
        length: u16,
        name: *const c_char,
    ) -> Cookie;
    fn xcb_intern_atom_reply(
        connection: *mut c_void,
        cookie: Cookie,
        error: *mut *mut c_void,
    ) -> *mut AtomReply;
    fn xcb_get_property(
        connection: *mut c_void,
        delete: u8,
        window: u32,
        atom: u32,
        kind: u32,
        offset: u32,
        length: u32,
    ) -> Cookie;
    fn xcb_get_property_reply(
        connection: *mut c_void,
        cookie: Cookie,
        error: *mut *mut c_void,
    ) -> *mut PropertyReply;
    fn xcb_get_property_value(reply: *const PropertyReply) -> *const c_void;
}

struct Reply<T>(NonNull<T>);
impl<T> Drop for Reply<T> {
    fn drop(&mut self) {
        // XCB allocates replies with libc malloc.
        unsafe {
            libc::free(self.0.as_ptr().cast());
        }
    }
}

pub(super) struct X11 {
    connection: NonNull<c_void>,
    root: u32,
    active: u32,
    gtk_id: u32,
}

impl X11 {
    pub fn open() -> io::Result<Self> {
        let mut screen = 0;
        // With a null name XCB connects to the current X11 display.
        let connection = NonNull::new(unsafe { xcb_connect(ptr::null(), &mut screen) })
            .ok_or_else(|| io::Error::other("Could not connect to X11"))?;
        let mut result = Self {
            connection,
            root: 0,
            active: 0,
            gtk_id: 0,
        };
        // The setup and screen data are borrowed from this open connection.
        unsafe {
            if xcb_connection_has_error(connection.as_ptr()) != 0 {
                return Err(io::Error::other("Could not connect to X11"));
            }
            let mut screens = xcb_setup_roots_iterator(xcb_get_setup(connection.as_ptr()));
            for _ in 0..screen {
                if screens.remaining <= 0 {
                    break;
                }
                xcb_screen_next(&mut screens);
            }
            if screens.remaining <= 0 || screens.data.is_null() {
                return Err(io::Error::other("Could not find the X11 screen"));
            }
            result.root = *screens.data;
        }
        result.active = result.atom(c"_NET_ACTIVE_WINDOW")?;
        result.gtk_id = result.atom(c"_GTK_APPLICATION_ID")?;
        Ok(result)
    }

    fn atom(&self, name: &CStr) -> io::Result<u32> {
        // The fixed names fit the XCB name length and remain valid through the request.
        unsafe {
            let connection = self.connection.as_ptr();
            let cookie =
                xcb_intern_atom(connection, 0, name.to_bytes().len() as u16, name.as_ptr());
            let reply = NonNull::new(xcb_intern_atom_reply(connection, cookie, ptr::null_mut()))
                .map(Reply)
                .ok_or_else(|| io::Error::other("X11 did not return a property identifier"))?;
            Ok(reply.0.as_ref().atom)
        }
    }

    fn property(&self, window: u32, atom: u32, kind: u32, format: u8) -> Option<Vec<u8>> {
        // Checked XCB replies return null if a window disappears. Bound the requested property.
        unsafe {
            let connection = self.connection.as_ptr();
            let cookie = xcb_get_property(connection, 0, window, atom, kind, 0, 256);
            let reply = NonNull::new(xcb_get_property_reply(connection, cookie, ptr::null_mut()))
                .map(Reply)?;
            let length = property_size(reply.0.as_ref(), format)?;
            let data = NonNull::new(
                xcb_get_property_value(reply.0.as_ptr())
                    .cast_mut()
                    .cast::<u8>(),
            )?;
            Some(std::slice::from_raw_parts(data.as_ptr(), length).to_vec())
        }
    }

    fn active_window(&self) -> Option<u32> {
        let data = self.property(self.root, self.active, 33, 32)?; // X11 WINDOW atom
        let window = u32::from_ne_bytes(data.try_into().ok()?);
        (window > 1).then_some(window)
    }

    pub fn focus(&self) -> Option<Focus> {
        let window = self.active_window()?;
        let app_id = self
            .property(window, self.gtk_id, 0, 8)
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .map(|mut value| {
                value.truncate(value.trim_end_matches('\0').len());
                value
            })
            .filter(|value| !value.is_empty());
        let classes = self
            .property(window, 67, 31, 8) // WM_CLASS, STRING
            .map(|bytes| {
                bytes
                    .split(|&byte| byte == 0)
                    .take(2)
                    .filter(|part| !part.is_empty())
                    .map(|part| String::from_utf8_lossy(part).into_owned())
                    .collect()
            })
            .unwrap_or_default();
        Some(Focus {
            window,
            app_id,
            classes,
        })
    }
}

impl Drop for X11 {
    fn drop(&mut self) {
        // This object owns the XCB connection.
        unsafe { xcb_disconnect(self.connection.as_ptr()) };
    }
}

#[link(name = "X11")]
unsafe extern "C" {
    fn XOpenDisplay(name: *const c_char) -> *mut c_void;
    fn XCloseDisplay(display: *mut c_void) -> c_int;
    fn XGetInputFocus(display: *mut c_void, focus: *mut c_ulong, revert: *mut c_int) -> c_int;
    fn XKeysymToKeycode(display: *mut c_void, keysym: c_ulong) -> c_uchar;
    fn XQueryKeymap(display: *mut c_void, keys: *mut c_char) -> c_int;
    fn XSync(display: *mut c_void, discard: c_int) -> c_int;
}
type FakeKey = unsafe extern "C" fn(*mut c_void, c_uint, c_int, c_ulong) -> c_int;
type Query =
    unsafe extern "C" fn(*mut c_void, *mut c_int, *mut c_int, *mut c_int, *mut c_int) -> c_int;

pub(super) struct Paste {
    display: NonNull<c_void>,
    library: NonNull<c_void>,
    fake: FakeKey,
}

impl Paste {
    pub fn open() -> io::Result<Self> {
        let error = || {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "X11 paste requires the XTEST extension",
            )
        };
        // Load only the known XTEST ABI and own both handles until drop.
        unsafe {
            let library = NonNull::new(libc::dlopen(
                c"libXtst.so.6".as_ptr(),
                libc::RTLD_NOW | libc::RTLD_LOCAL,
            ))
            .ok_or_else(error)?;
            let fake = NonNull::new(libc::dlsym(library.as_ptr(), c"XTestFakeKeyEvent".as_ptr()));
            let query = NonNull::new(libc::dlsym(
                library.as_ptr(),
                c"XTestQueryExtension".as_ptr(),
            ));
            let (Some(fake), Some(query)) = (fake, query) else {
                libc::dlclose(library.as_ptr());
                return Err(error());
            };
            let Some(display) = NonNull::new(XOpenDisplay(ptr::null())) else {
                libc::dlclose(library.as_ptr());
                return Err(error());
            };
            let query: Query = std::mem::transmute(query.as_ptr());
            let (mut event, mut fault, mut major, mut minor) = (0, 0, 0, 0);
            let result = Self {
                display,
                library,
                fake: std::mem::transmute::<*mut c_void, FakeKey>(fake.as_ptr()),
            };
            if query(
                display.as_ptr(),
                &mut event,
                &mut fault,
                &mut major,
                &mut minor,
            ) == 0
            {
                return Err(error());
            }
            Ok(result)
        }
    }

    pub fn focus(&self) -> Option<u64> {
        let (mut focus, mut revert) = (0, 0);
        // XGetInputFocus only reads the focus target for this display.
        let success = unsafe { XGetInputFocus(self.display.as_ptr(), &mut focus, &mut revert) };
        if success != 0 && focus > 1 {
            Some(focus as _)
        } else {
            None
        }
    }

    pub fn paste(&self, target: u64) -> io::Result<()> {
        let display = self.display.as_ptr();
        // Query the keyboard before sending Ctrl+V and preserve a physically held Ctrl.
        unsafe {
            if self.focus() != Some(target) {
                return Err(io::Error::other(
                    "Focus changed. The text is ready on the clipboard",
                ));
            }
            let mut keys = [0_u8; 32];
            XQueryKeymap(display, keys.as_mut_ptr().cast());
            let down = |key: u8| keys[usize::from(key / 8)] & (1 << (key % 8)) != 0;
            let keycode = |symbol| XKeysymToKeycode(display, symbol);
            let ctrl = keycode(0xffe3);
            let v = keycode(0x76);
            let modifiers = [0xffe1, 0xffe2, 0xffe9, 0xffea, 0xffeb, 0xffec].map(keycode);
            if ctrl == 0
                || v == 0
                || down(v)
                || modifiers.into_iter().filter(|&code| code != 0).any(down)
            {
                return Err(io::Error::other(
                    "The text is ready on the clipboard. Paste it after releasing the keys",
                ));
            }
            let synthesize_ctrl = !(down(ctrl) || down(keycode(0xffe4)));
            let mut okay = true;
            let mut send = |key, press: bool| {
                okay &= (self.fake)(display, u32::from(key), c_int::from(press), 0) != 0;
            };
            if synthesize_ctrl {
                send(ctrl, true);
            }
            send(v, true);
            send(v, false);
            if synthesize_ctrl {
                send(ctrl, false);
            }
            XSync(display, 0);
            if okay {
                Ok(())
            } else {
                Err(io::Error::other("X11 rejected the paste shortcut"))
            }
        }
    }
}

impl Drop for Paste {
    fn drop(&mut self) {
        // Close the display before dropping the library that installed its XTEST hooks.
        unsafe {
            XCloseDisplay(self.display.as_ptr());
            libc::dlclose(self.library.as_ptr());
        }
    }
}

#[cfg(test)]
pub(in crate::ui) fn test_focus(window: &gtk::Window) -> u32 {
    use gtk::glib::translate::ToGlibPtr;
    use gtk::prelude::*;

    #[link(name = "gtk-4")]
    unsafe extern "C" {
        fn gdk_x11_surface_get_xid(surface: *mut gtk::gdk::ffi::GdkSurface) -> c_ulong;
    }
    #[link(name = "X11")]
    unsafe extern "C" {
        fn XDefaultRootWindow(display: *mut c_void) -> c_ulong;
        fn XInternAtom(display: *mut c_void, name: *const c_char, existing: c_int) -> c_ulong;
        fn XChangeProperty(
            display: *mut c_void,
            window: c_ulong,
            property: c_ulong,
            kind: c_ulong,
            format: c_int,
            mode: c_int,
            data: *const u8,
            elements: c_int,
        ) -> c_int;
        fn XSetInputFocus(
            display: *mut c_void,
            window: c_ulong,
            revert: c_int,
            time: c_ulong,
        ) -> c_int;
    }

    let surface = window.surface().expect("mapped fixture");
    let app = b"org.dragondriver.OfflineFixture";
    // This helper only runs on the isolated X11 test display and only targets the fixture window.
    unsafe {
        let display = XOpenDisplay(ptr::null());
        assert!(!display.is_null());
        let window = gdk_x11_surface_get_xid(surface.to_glib_none().0);
        let active = XInternAtom(display, c"_NET_ACTIVE_WINDOW".as_ptr(), 0);
        let gtk_id = XInternAtom(display, c"_GTK_APPLICATION_ID".as_ptr(), 0);
        let utf8 = XInternAtom(display, c"UTF8_STRING".as_ptr(), 0);
        let root = XDefaultRootWindow(display);
        XChangeProperty(
            display,
            root,
            active,
            33,
            32,
            0,
            (&window as *const c_ulong).cast(),
            1,
        );
        XChangeProperty(
            display,
            window,
            gtk_id,
            utf8,
            8,
            0,
            app.as_ptr(),
            app.len() as c_int,
        );
        XSetInputFocus(display, window, 0, 0);
        XSync(display, 0);
        XCloseDisplay(display);
        window.try_into().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xcb_property_values_fit_the_wire_payload() {
        let mut reply = PropertyReply {
            response_type: 1,
            format: 8,
            sequence: 0,
            length: 2,
            kind: 31,
            bytes_after: 0,
            value_len: 7,
        };
        assert_eq!(property_size(&reply, 8), Some(7));
        reply.length = 1;
        assert_eq!(property_size(&reply, 8), None);
        reply.format = 32;
        reply.value_len = 1;
        assert_eq!(property_size(&reply, 32), Some(4));
        reply.value_len = u32::MAX;
        assert_eq!(property_size(&reply, 32), None);
    }
}
