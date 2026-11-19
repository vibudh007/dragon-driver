use std::io::{self, Write};
use xml::reader::{ParserConfig2, XmlEvent as ReadEvent};
use xml::writer::{EmitterConfig, EventWriter, XmlEvent as WriteEvent};

mod bulk;
mod custom;
mod macro_records;
mod pos_events;
pub use custom::{CustomLightInfo, CustomLightKey, KeyAction};
pub use macro_records::{MacroDirection, MacroInfo, MacroUpdate, Occurrence};
pub use pos_events::PosEvent;

const MAX_XML_BYTES: usize = 16 * 1024 * 1024;
const MAX_DEPTH: usize = 64;
const MAX_XML_NODES: usize = 131_072;
const MAX_XML_ATTRIBUTES: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq)]
enum Child {
    Element(Element),
    Text(String),
    Comment(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Element {
    name: String,
    attributes: Vec<(String, String)>,
    children: Vec<Child>,
}

impl Element {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            attributes: Vec::new(),
            children: Vec::new(),
        }
    }

    fn attribute(&self, key: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    fn set(&mut self, key: &str, value: impl Into<String>) {
        if let Some((_, previous)) = self.attributes.iter_mut().find(|(name, _)| name == key) {
            *previous = value.into();
        } else {
            self.attributes.push((key.into(), value.into()));
        }
    }

    fn append(&mut self, child: Element) {
        self.children.push(Child::Element(child));
    }

    fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|child| match child {
            Child::Element(element) => Some(element),
            _ => None,
        })
    }

    fn elements_mut(&mut self) -> impl Iterator<Item = &mut Element> {
        self.children.iter_mut().filter_map(|child| match child {
            Child::Element(element) => Some(element),
            _ => None,
        })
    }

    fn child(&self, name: &str) -> Option<&Element> {
        self.elements().find(|child| child.name == name)
    }

    fn child_mut(&mut self, name: &str) -> Option<&mut Element> {
        self.elements_mut().find(|child| child.name == name)
    }

    fn replace_child(&mut self, replacement: Element) -> bool {
        let Some(index) = self
            .children
            .iter()
            .position(|item| matches!(item, Child::Element(node) if node.name == replacement.name))
        else {
            return false;
        };
        self.children.remove(index);
        self.append(replacement);
        true
    }

    fn write_xml(&self, writer: &mut EventWriter<impl Write>) -> Result<(), xml::writer::Error> {
        let mut start = WriteEvent::start_element(self.name.as_str());
        for (key, value) in &self.attributes {
            validate_xml_text(value)?;
            start = start.attr(key.as_str(), value);
        }
        writer.write(start)?;
        for child in &self.children {
            match child {
                Child::Element(element) => element.write_xml(writer)?,
                Child::Text(text) => {
                    validate_xml_text(text)?;
                    writer.write(WriteEvent::characters(text))?;
                }
                Child::Comment(text) => {
                    validate_xml_text(text)?;
                    writer.write(WriteEvent::comment(text))?;
                }
            }
        }
        writer.write(WriteEvent::end_element())
    }
}

fn validate_xml_text(text: &str) -> Result<(), xml::writer::Error> {
    if text.chars().any(|character| {
        !matches!(character, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
    }) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid XML 1.0 character").into());
    }
    Ok(())
}

#[derive(Clone)]
pub struct Profiles {
    root: Element,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SleepSettings {
    pub lights: Option<i32>,
    pub keyboard: Option<i32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MacroEvent<'document> {
    pub mark: &'document str,
    pub delay: &'document str,
    pub event_type: &'document str,
    pub key_code: &'document str,
    pub code_name: &'document str,
    pub explanation: &'document str,
}

const FUNCTION_KEYS: [&str; 4] = ["feature1", "feature2", "feature3", "feature4"];
const STROKE_KEYS: [&str; 4] = ["Value1", "Value2", "Value3", "Value4"];
const KEY_CODES: [&str; 4] = ["keyCode1", "keyCode2", "keyCode3", "keyCode4"];
const STAGE_KEYS: [[&str; 4]; 4] = [
    ["K1A", "K2A", "K3A", "K4A"],
    ["K1B", "K2B", "K3B", "K4B"],
    ["K1C", "K2C", "K3C", "K4C"],
    ["K1D", "K2D", "K3D", "K4D"],
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdvancedKind {
    Dks,
    ModTap,
    Toggle,
}

impl AdvancedKind {
    fn container(self) -> &'static str {
        match self {
            Self::Dks => "configDKS",
            Self::ModTap => "configMT",
            Self::Toggle => "configTGL",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdvancedAction<'document> {
    Dks {
        key_codes: [&'document str; 4],
        stages: [[i32; 4]; 4],
        values: [i32; 4],
    },
    ModTap {
        key_codes: [&'document str; 2],
        value_time: i32,
    },
    Toggle {
        key_code: &'document str,
        status: i32,
    },
}

impl AdvancedAction<'_> {
    fn kind(&self) -> AdvancedKind {
        match self {
            Self::Dks { .. } => AdvancedKind::Dks,
            Self::ModTap { .. } => AdvancedKind::ModTap,
            Self::Toggle { .. } => AdvancedKind::Toggle,
        }
    }

    fn write(self, node: &mut Element) {
        match self {
            Self::Dks {
                key_codes,
                stages,
                values,
            } => {
                for (position, code) in key_codes.into_iter().enumerate() {
                    node.set(KEY_CODES[position], code);
                    for (key, value) in STAGE_KEYS[position].into_iter().zip(stages[position]) {
                        node.set(key, value.to_string());
                    }
                }
                for (key, value) in STROKE_KEYS.into_iter().zip(values) {
                    node.set(key, value.to_string());
                }
            }
            Self::ModTap {
                key_codes,
                value_time,
            } => {
                node.set(KEY_CODES[0], key_codes[0]);
                node.set(KEY_CODES[1], key_codes[1]);
                node.set("ValueTime", value_time.to_string());
            }
            Self::Toggle { key_code, status } => {
                node.set(KEY_CODES[0], key_code);
                node.set("status", status.to_string());
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdvancedItem<'document> {
    pub index: i32,
    pub name: &'document str,
    pub action: AdvancedAction<'document>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileInfo<'document> {
    pub index: &'document str,
    pub name: &'document str,
    pub application: &'document str,
    pub icon: &'document str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LightBank {
    Keyboard,
    SideOne,
    SideTwo,
}

impl LightBank {
    fn element(self) -> &'static str {
        match self {
            Self::Keyboard => "light0",
            Self::SideOne => "light1",
            Self::SideTwo => "light2",
        }
    }
}

const LIGHT_ITEM_KEYS: [&str; 14] = [
    "ledKeyboardOrSide",
    "ledType",
    "ledBrightness",
    "ledSpeed",
    "ledModeR",
    "ledModeG",
    "ledModeB",
    "ledBgR",
    "ledBgG",
    "ledBgB",
    "ledDirection",
    "ledModeType",
    "ledOpenOrClose",
    "ledUpdate",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LightItem {
    pub region: i32,
    pub mode: i32,
    pub brightness: i32,
    pub speed: i32,
    pub foreground: [i32; 3],
    pub background: [i32; 3],
    pub direction: i32,
    pub mode_type: i32,
    pub open: i32,
    pub update: i32,
}

impl LightItem {
    fn from_node(node: &Element) -> Self {
        let [
            region,
            mode,
            brightness,
            speed,
            r,
            g,
            b,
            bg_r,
            bg_g,
            bg_b,
            direction,
            mode_type,
            open,
            update,
        ] = LIGHT_ITEM_KEYS.map(|key| node.attribute(key).map_or(0, decimal));
        Self {
            region,
            mode,
            brightness,
            speed,
            foreground: [r, g, b],
            background: [bg_r, bg_g, bg_b],
            direction,
            mode_type,
            open,
            update,
        }
    }

    fn write(self, node: &mut Element) {
        let [r, g, b] = self.foreground;
        let [bg_r, bg_g, bg_b] = self.background;
        for (key, value) in LIGHT_ITEM_KEYS.into_iter().zip([
            self.region,
            self.mode,
            self.brightness,
            self.speed,
            r,
            g,
            b,
            bg_r,
            bg_g,
            bg_b,
            self.direction,
            self.mode_type,
            self.open,
            self.update,
        ]) {
            node.set(key, value.to_string());
        }
    }
}

impl Default for Profiles {
    fn default() -> Self {
        let mut root = Element::new("Parameters");
        root.set("profileIndex", "1");
        Self { root }
    }
}

impl Profiles {
    pub fn from_xml(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() > MAX_XML_BYTES {
            return Err(invalid_xml("profile XML exceeds the input limit"));
        }
        let mut parser = ParserConfig2::new()
            .allow_multiple_root_elements(false)
            .ignore_comments(false)
            .max_attributes(MAX_XML_ATTRIBUTES)
            .max_data_length(MAX_XML_BYTES)
            .max_entity_expansion_length(4096)
            .max_entity_expansion_depth(4)
            .create_reader(bytes);
        let mut root = None;
        let mut stack: Vec<Element> = Vec::new();
        let mut nodes = 0;
        loop {
            let event = parser.next().map_err(invalid_xml)?;
            if parser.doctype().is_some() {
                return Err(invalid_xml(
                    "document type declarations are unsupported in profiles",
                ));
            }
            if matches!(
                &event,
                ReadEvent::StartElement { .. }
                    | ReadEvent::Characters(_)
                    | ReadEvent::Whitespace(_)
                    | ReadEvent::CData(_)
                    | ReadEvent::Comment(_)
            ) {
                nodes += 1;
                if nodes > MAX_XML_NODES {
                    return Err(invalid_xml("profile XML exceeds the node limit"));
                }
            }
            match event {
                ReadEvent::EndDocument => break,
                ReadEvent::StartElement {
                    name, attributes, ..
                } => {
                    if stack.len() >= MAX_DEPTH
                        || name.namespace.is_some()
                        || attributes.iter().any(|attr| attr.name.namespace.is_some())
                    {
                        return Err(invalid_xml(
                            "profile XML exceeds nesting limits or uses namespaces",
                        ));
                    }
                    let mut element = Element::new(name.local_name);
                    element.attributes = attributes
                        .into_iter()
                        .map(|attr| (attr.name.local_name, attr.value))
                        .collect();
                    stack.push(element);
                }
                ReadEvent::EndElement { .. } => {
                    let element = stack
                        .pop()
                        .ok_or_else(|| invalid_xml("unmatched profile XML element"))?;
                    if let Some(parent) = stack.last_mut() {
                        parent.append(element);
                    } else {
                        root = Some(element);
                    }
                }
                ReadEvent::Characters(text)
                | ReadEvent::Whitespace(text)
                | ReadEvent::CData(text) => {
                    if let Some(element) = stack.last_mut() {
                        element.children.push(Child::Text(text));
                    }
                }
                ReadEvent::Comment(text) => {
                    if let Some(element) = stack.last_mut() {
                        element.children.push(Child::Comment(text));
                    }
                }
                _ => {}
            }
        }
        let root = root
            .filter(|node| node.name == "Parameters")
            .ok_or_else(|| invalid_xml("missing Parameters root"))?;
        Ok(Self { root })
    }

    pub fn to_xml(&self) -> Result<Vec<u8>, xml::writer::Error> {
        let mut output = Vec::new();
        let mut writer = EmitterConfig::new().create_writer(&mut output);
        self.root.write_xml(&mut writer)?;
        Ok(output)
    }

    pub fn profile_index(&self) -> i32 {
        self.root.attribute("profileIndex").map_or(1, decimal)
    }

    pub fn set_profile_index(&mut self, index: i32) {
        self.root.set("profileIndex", index.to_string());
    }

    pub fn append_profile(&mut self, index: &str, name: &str, link_lights: bool) {
        let mut profile = Element::new("config");
        for (key, value) in [
            ("index", index),
            ("name", name),
            ("proFileAppName", ""),
            ("proFileAppIco", ""),
        ] {
            profile.set(key, value);
        }
        for name in ["light0", "light1", "light2"] {
            let mut light = Element::new(name);
            light.set("usingIndex", "2");
            if name == "light0" {
                light.set("ledOpenOrClose", "0");
                light.set("SynchronizeStatus", "0");
            } else if link_lights {
                light.set("linkStatus", "0");
            }
            profile.append(light);
        }
        for name in [
            "customColor",
            "customLight",
            "ribbon",
            "onboardBinding",
            "keyEvent",
            "dpi",
        ] {
            profile.append(Element::new(name));
        }
        let mut function = Element::new("function");
        let mut settings = Element::new("Setting");
        for (name, value) in FUNCTION_KEYS.into_iter().zip(["0", "0", "0", "1"]) {
            settings.set(name, value);
        }
        function.append(settings);
        let mut sleep = Element::new("SleepTime");
        sleep.set("lightSleepTime", "600");
        sleep.set("keyboardSleepTime", "30");
        function.append(sleep);
        profile.append(function);
        profile.append(Element::new("bindMacro"));
        self.root.append(profile);
    }

    pub fn contains(&self, index: &str) -> bool {
        self.config(index).is_some()
    }

    pub fn rename_profile(&mut self, index: &str, name: &str) -> bool {
        let Some(profile) = self.config_mut(index) else {
            return false;
        };
        profile.set("name", name);
        profile.set("proFileAppName", "");
        profile.set("proFileAppIco", "");
        true
    }

    pub fn set_profile_application(
        &mut self,
        index: &str,
        application: &str,
        x11_class: Option<&str>,
    ) -> bool {
        let Some(profile) = self.config_mut(index) else {
            return false;
        };
        profile.set("proFileAppName", application);
        profile.set("proFileAppIco", "");
        profile.set("linuxStartupWmClass", x11_class.unwrap_or_default());
        true
    }

    pub fn profile_x11_class(&self, index: &str) -> Option<&str> {
        self.config(index)?
            .attribute("linuxStartupWmClass")
            .filter(|class| !class.is_empty())
    }

    pub fn profile(&self, index: &str) -> Option<ProfileInfo<'_>> {
        self.config(index).map(profile_info)
    }

    pub fn list_profiles(&self) -> Vec<ProfileInfo<'_>> {
        let mut entries: Vec<_> = self
            .root
            .elements()
            .filter(|node| node.name == "config")
            .map(profile_info)
            .collect();
        entries.sort_by_key(|entry| decimal(entry.index));
        entries
    }

    pub fn remove_profile(&mut self, index: &str) -> bool {
        remove_indexed(&mut self.root, "config", index)
    }

    pub fn export_profile(
        &self,
        index: &str,
        destination: &mut Self,
        increment_index: bool,
    ) -> bool {
        let Some(mut node) = self.config(index).cloned() else {
            return false;
        };
        if increment_index {
            let Some(new_index) = decimal(index).checked_add(1) else {
                return false;
            };
            node.set("index", new_index.to_string());
        }
        destination.root.append(node);
        true
    }

    pub fn import_profile(&mut self, source: &Self, index: &str) -> Option<i32> {
        let node = source.config(index)?;
        let last = self
            .root
            .elements()
            .filter(|node| node.name == "config")
            .map(|node| node.attribute("index").map_or(0, decimal))
            .fold(-1, i32::max);
        let new_index = last.checked_add(1)?;
        let mut node = node.clone();
        node.set("index", new_index.to_string());
        self.root.append(node);
        Some(new_index)
    }

    pub fn light_using_index(&self, index: &str, bank: LightBank) -> Option<i32> {
        Some(decimal(
            self.config(index)?
                .child(bank.element())?
                .attribute("usingIndex")?,
        ))
    }

    pub fn set_light_using_index(&mut self, index: &str, bank: LightBank, value: i32) -> bool {
        self.set_light_attribute(index, bank, "usingIndex", value)
    }

    pub fn keyboard_light_status(&self, index: &str) -> Option<(i32, i32)> {
        let node = self.config(index)?.child(LightBank::Keyboard.element())?;
        Some((
            node.attribute("ledOpenOrClose").map_or(0, decimal),
            node.attribute("SynchronizeStatus").map_or(0, decimal),
        ))
    }

    pub fn set_keyboard_light_open(&mut self, index: &str, value: i32) -> bool {
        self.set_light_attribute(index, LightBank::Keyboard, "ledOpenOrClose", value)
    }

    pub fn set_keyboard_light_sync(&mut self, index: &str, value: i32) -> bool {
        self.set_light_attribute(index, LightBank::Keyboard, "SynchronizeStatus", value)
    }

    pub fn lights(&self, index: &str, bank: LightBank) -> Option<Vec<LightItem>> {
        let bank = self.config(index)?.child(bank.element())?;
        Some(
            bank.elements()
                .filter(|node| node.name == "item")
                .map(LightItem::from_node)
                .collect(),
        )
    }

    pub fn contains_light_mode(&self, index: &str, bank: LightBank, mode: i32) -> bool {
        self.config(index)
            .and_then(|node| node.child(bank.element()))
            .is_some_and(|bank| {
                bank.elements()
                    .filter_map(|node| node.attribute("ledType"))
                    .any(|stored| decimal(stored) == mode)
            })
    }

    pub fn add_light(&mut self, index: &str, bank: LightBank, item: LightItem) -> bool {
        let Some(bank) = self
            .config_mut(index)
            .and_then(|node| node.child_mut(bank.element()))
        else {
            return false;
        };
        let mut node = Element::new("item");
        item.write(&mut node);
        bank.append(node);
        true
    }

    pub fn modify_light(
        &mut self,
        index: &str,
        bank: LightBank,
        mode: i32,
        item: LightItem,
    ) -> bool {
        let Some(node) = self
            .config_mut(index)
            .and_then(|node| node.child_mut(bank.element()))
            .and_then(|bank| {
                bank.elements_mut().find(|node| {
                    node.name == "item" && node.attribute("ledType").map(decimal) == Some(mode)
                })
            })
        else {
            return false;
        };
        item.write(node);
        true
    }

    fn set_light_attribute(&mut self, index: &str, bank: LightBank, key: &str, value: i32) -> bool {
        let Some(node) = self
            .config_mut(index)
            .and_then(|node| node.child_mut(bank.element()))
        else {
            return false;
        };
        node.set(key, value.to_string());
        true
    }

    pub fn function_settings(&self, index: &str) -> Option<[Option<&str>; 4]> {
        let settings = self.config(index)?.child("function")?.child("Setting")?;
        Some(FUNCTION_KEYS.map(|name| settings.attribute(name)))
    }

    pub fn set_function_settings(&mut self, index: &str, features: [&str; 4]) -> bool {
        let Some(function) = self
            .config_mut(index)
            .and_then(|node| node.child_mut("function"))
        else {
            return false;
        };
        let mut settings = Element::new("Setting");
        for (name, value) in FUNCTION_KEYS.into_iter().zip(features) {
            settings.set(name, value);
        }
        function.replace_child(settings)
    }

    pub fn sleep_settings(&self, index: &str) -> Option<SleepSettings> {
        let settings = self.config(index)?.child("function")?.child("SleepTime")?;
        Some(SleepSettings {
            lights: settings.attribute("lightSleepTime").map(decimal),
            keyboard: settings.attribute("keyboardSleepTime").map(decimal),
        })
    }

    pub fn set_sleep_settings(&mut self, index: &str, lights: i32, keyboard: i32) -> bool {
        let Some(function) = self
            .config_mut(index)
            .and_then(|node| node.child_mut("function"))
        else {
            return false;
        };
        let mut settings = Element::new("SleepTime");
        settings.set("lightSleepTime", lights.to_string());
        settings.set("keyboardSleepTime", keyboard.to_string());
        function.replace_child(settings)
    }

    pub fn append_macro(&mut self, index: &str, name: &str) {
        let mut node = Element::new("macro");
        node.set("index", index);
        node.set("name", name);
        self.root.append(node);
    }

    pub fn macro_events(&self, index: &str) -> Option<Vec<MacroEvent<'_>>> {
        let node = self
            .root
            .elements()
            .find(|node| indexed(node, "macro", index))?;
        Some(macro_records::events(node))
    }

    pub fn append_macro_events(&mut self, index: &str, events: &[MacroEvent<'_>]) -> bool {
        let Some(node) = self
            .root
            .elements_mut()
            .find(|node| indexed(node, "macro", index))
        else {
            return false;
        };
        macro_records::append_events(node, events);
        true
    }

    pub fn advanced_document() -> Self {
        let mut root = Element::new("Parameters");
        for (kind, name, key_type) in [
            (AdvancedKind::Dks, "SeniorKeyDKS", "1"),
            (AdvancedKind::ModTap, "SeniorKeyMT", "2"),
            (AdvancedKind::Toggle, "SeniorKeyTGL", "3"),
        ] {
            let mut container = Element::new(kind.container());
            container.set("name", name);
            container.set("keyType", key_type);
            root.append(container);
        }
        let mut stroke = Element::new("strokeNum");
        let mut values = Element::new("DKSValue");
        for (key, value) in STROKE_KEYS.into_iter().zip(["15", "36", "36", "15"]) {
            values.set(key, value);
        }
        stroke.append(values);
        root.append(stroke);
        Self { root }
    }

    pub fn with_advanced_definitions(mut self, source: &Self) -> Self {
        for name in ["configDKS", "configMT", "configTGL", "strokeNum"] {
            self.root
                .children
                .retain(|child| !matches!(child, Child::Element(element) if element.name == name));
            if let Some(element) = source.root.child(name) {
                self.root.append(element.clone());
            }
        }
        self
    }

    pub fn advanced_definitions(&self) -> Option<Self> {
        self.root.child("configDKS")?;
        Some(Self::advanced_document().with_advanced_definitions(self))
    }

    pub fn dks_stroke_values(&self) -> Option<[Option<i32>; 4]> {
        let values = self.root.child("strokeNum")?.child("DKSValue")?;
        Some(STROKE_KEYS.map(|key| values.attribute(key).map(decimal)))
    }

    pub fn set_dks_stroke_values(&mut self, stroke: [i32; 4]) -> bool {
        let Some(values) = self
            .root
            .child_mut("strokeNum")
            .and_then(|node| node.child_mut("DKSValue"))
        else {
            return false;
        };
        for (key, value) in STROKE_KEYS.into_iter().zip(stroke) {
            values.set(key, value.to_string());
        }
        true
    }

    pub fn advanced_items(&self, kind: AdvancedKind) -> Option<Vec<AdvancedItem<'_>>> {
        let container = self.root.child(kind.container())?;
        Some(
            container
                .elements()
                .filter(|node| node.name == "item")
                .map(|node| {
                    let text = |key| node.attribute(key).unwrap_or_default();
                    let number = |key| decimal(text(key));
                    let action = match kind {
                        AdvancedKind::Dks => AdvancedAction::Dks {
                            key_codes: KEY_CODES.map(text),
                            stages: STAGE_KEYS.map(|stage| stage.map(number)),
                            values: STROKE_KEYS.map(number),
                        },
                        AdvancedKind::ModTap => AdvancedAction::ModTap {
                            key_codes: [text(KEY_CODES[0]), text(KEY_CODES[1])],
                            value_time: number("ValueTime"),
                        },
                        AdvancedKind::Toggle => AdvancedAction::Toggle {
                            key_code: text(KEY_CODES[0]),
                            status: number("status"),
                        },
                    };
                    AdvancedItem {
                        index: number("index"),
                        name: text("name"),
                        action,
                    }
                })
                .collect(),
        )
    }

    pub fn contains_advanced_index(&self, index: i32) -> bool {
        for kind in [
            AdvancedKind::Dks,
            AdvancedKind::ModTap,
            AdvancedKind::Toggle,
        ] {
            let Some(container) = self.root.child(kind.container()) else {
                continue;
            };
            if container
                .elements()
                .filter_map(|node| node.attribute("index"))
                .any(|stored| decimal(stored) == index)
            {
                return true;
            }
        }
        false
    }

    pub fn append_advanced_item(&mut self, item: AdvancedItem<'_>) -> bool {
        let Some(container) = self.root.child_mut(item.action.kind().container()) else {
            return false;
        };
        let mut node = Element::new("item");
        node.set("index", item.index.to_string());
        node.set("name", item.name);
        item.action.write(&mut node);
        container.append(node);
        true
    }

    pub fn modify_advanced_item(&mut self, item: AdvancedItem<'_>) -> bool {
        let Some(node) = self
            .root
            .child_mut(item.action.kind().container())
            .and_then(|container| {
                container
                    .elements_mut()
                    .find(|node| advanced_index(node, item.index))
            })
        else {
            return false;
        };
        node.set("name", item.name);
        item.action.write(node);
        true
    }

    pub fn remove_advanced_item(&mut self, kind: AdvancedKind, index: i32) -> bool {
        let Some(container) = self.root.child_mut(kind.container()) else {
            return false;
        };
        let Some(position) = container
            .children
            .iter()
            .position(|child| matches!(child, Child::Element(node) if advanced_index(node, index)))
        else {
            return false;
        };
        container.children.remove(position);
        true
    }

    fn config(&self, index: &str) -> Option<&Element> {
        self.root
            .elements()
            .find(|node| indexed(node, "config", index))
    }

    fn config_mut(&mut self, index: &str) -> Option<&mut Element> {
        self.root
            .elements_mut()
            .find(|node| indexed(node, "config", index))
    }
}

fn indexed(node: &Element, name: &str, index: &str) -> bool {
    node.name == name && node.attribute("index") == Some(index)
}

fn profile_info(node: &Element) -> ProfileInfo<'_> {
    let text = |key| node.attribute(key).unwrap_or_default();
    ProfileInfo {
        index: text("index"),
        name: text("name"),
        application: text("proFileAppName"),
        icon: text("proFileAppIco"),
    }
}

fn remove_indexed(parent: &mut Element, name: &str, index: &str) -> bool {
    let Some(position) = parent
        .children
        .iter()
        .position(|child| matches!(child, Child::Element(node) if indexed(node, name, index)))
    else {
        return false;
    };
    parent.children.remove(position);
    true
}

fn advanced_index(node: &Element, index: i32) -> bool {
    node.name == "item" && node.attribute("index").map(decimal) == Some(index)
}

fn invalid_xml(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

pub(super) fn decimal(value: &str) -> i32 {
    let value = value.trim_start_matches(|character: char| character.is_ascii_whitespace());
    let (sign, digits) = if let Some(digits) = value.strip_prefix('-') {
        (-1, digits)
    } else {
        (1, value.strip_prefix('+').unwrap_or(value))
    };
    let number = digits
        .bytes()
        .take_while(u8::is_ascii_digit)
        .fold(0_i64, |value, digit| {
            value
                .saturating_mul(10)
                .saturating_add(i64::from(digit - b'0'))
        });
    (sign * number).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advanced_indices_are_found_in_partial_documents() {
        let profiles = Profiles::from_xml(
            br#"<Parameters><configTGL><item index="6"/></configTGL></Parameters>"#,
        )
        .unwrap();
        assert!(profiles.contains_advanced_index(6));
        assert!(!profiles.contains_advanced_index(7));
    }

    #[test]
    fn creates_vendor_profile_defaults_and_escapes_names() {
        let mut profiles = Profiles::default();
        profiles.append_profile("3", "Alice & Bob", true);
        let output = String::from_utf8(profiles.to_xml().unwrap()).unwrap();
        assert!(output.contains("profileIndex=\"1\""));
        assert!(output.contains("name=\"Alice &amp; Bob\""));
        assert!(output.contains("<light1 usingIndex=\"2\" linkStatus=\"0\""));
        assert!(output.contains("feature4=\"1\""));
        assert!(output.contains("<SleepTime lightSleepTime=\"600\" keyboardSleepTime=\"30\""));
        assert!(Profiles::from_xml(output.as_bytes()).unwrap().contains("3"));
    }

    #[test]
    fn edits_existing_profiles_without_dropping_unknown_settings() {
        let mut profiles = Profiles::from_xml(br#"<Parameters profileIndex="  +4suffix"><config index="4" name="old" proFileAppIco="icon"><extra>&lt;</extra></config></Parameters>"#).unwrap();
        assert_eq!(profiles.profile_index(), 4);
        assert!(!profiles.rename_profile("5", "no"));
        assert!(profiles.rename_profile("4", "new"));
        profiles.set_profile_index(-1);
        let output = String::from_utf8(profiles.to_xml().unwrap()).unwrap();
        assert!(output.contains("profileIndex=\"-1\""));
        assert!(output.contains("name=\"new\" proFileAppIco=\"\""));
        assert!(output.contains("<extra>&lt;</extra>"));
    }

    #[test]
    fn rejects_truncated_or_deep_xml() {
        assert!(Profiles::from_xml(b"<Parameters>").is_err());
        let data = format!(
            "<Parameters>{}{}</Parameters>",
            "<x>".repeat(MAX_DEPTH),
            "</x>".repeat(MAX_DEPTH)
        );
        assert!(Profiles::from_xml(data.as_bytes()).is_err());
    }

    #[test]
    fn round_trips_profile_settings_and_macro_events() {
        let event = MacroEvent {
            mark: "1",
            delay: "10",
            event_type: "0",
            key_code: "4",
            code_name: "A",
            explanation: "press & hold",
        };
        let mut profiles = Profiles::default();
        profiles.append_macro("1", "macro");
        profiles.append_profile("1", "profile", false);
        assert!(profiles.append_macro_events("1", &[event]));
        assert!(profiles.set_sleep_settings("1", 500, 0));
        assert!(profiles.set_function_settings("1", ["2", "3", "5", "7"]));

        let document = Profiles::from_xml(&profiles.to_xml().unwrap()).unwrap();
        assert_eq!(document.macro_events("1").unwrap(), vec![event]);
        assert_eq!(
            document.sleep_settings("1"),
            Some(SleepSettings {
                lights: Some(500),
                keyboard: Some(0)
            })
        );
        assert_eq!(
            document.function_settings("1"),
            Some([Some("2"), Some("3"), Some("5"), Some("7")])
        );
    }

    #[test]
    fn round_trips_vendor_advanced_key_document() {
        let mut document = Profiles::advanced_document();
        assert_eq!(
            document.dks_stroke_values(),
            Some([Some(15), Some(36), Some(36), Some(15)])
        );
        assert!(document.set_dks_stroke_values([3, 6, 9, 12]));
        let item = AdvancedItem {
            index: 7,
            name: "DKS",
            action: AdvancedAction::Dks {
                key_codes: ["A", "B", "C", "D"],
                stages: [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]],
                values: [3, 6, 9, 12],
            },
        };
        assert!(document.append_advanced_item(item));
        let mut document = Profiles::from_xml(&document.to_xml().unwrap()).unwrap();
        assert_eq!(
            document.advanced_items(AdvancedKind::Dks).unwrap(),
            vec![item]
        );
        assert!(document.modify_advanced_item(AdvancedItem {
            name: "changed",
            ..item
        }));
        assert_eq!(
            document.advanced_items(AdvancedKind::Dks).unwrap()[0].name,
            "changed"
        );
        assert!(document.remove_advanced_item(AdvancedKind::Dks, 7));
        assert!(
            document
                .advanced_items(AdvancedKind::Dks)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn imports_with_the_next_numeric_profile_index() {
        let mut source = Profiles::default();
        source.append_profile("8", "name", false);
        let mut archive = Profiles::default();
        assert!(source.export_profile("8", &mut archive, true));
        assert_eq!(archive.profile("9").unwrap().name, "name");
        let mut destination = Profiles::default();
        assert_eq!(destination.import_profile(&archive, "9"), Some(0));
        destination.append_profile("10", "ten", false);
        destination.append_profile("2", "two", false);
        assert_eq!(destination.import_profile(&archive, "9"), Some(11));
        assert!(destination.remove_profile("2"));
        assert_eq!(
            destination
                .list_profiles()
                .into_iter()
                .map(|info| info.index)
                .collect::<Vec<_>>(),
            ["0", "10", "11"]
        );
    }

    #[test]
    fn stores_lighting_by_bank_and_mode() {
        let mut profiles = Profiles::default();
        profiles.append_profile("1", "fixture", true);
        let item = LightItem {
            region: 0,
            mode: 3,
            brightness: 5,
            speed: 2,
            foreground: [255, 1, 2],
            background: [4, 5, 6],
            direction: 1,
            mode_type: 2,
            open: 1,
            update: 0,
        };
        assert!(profiles.add_light("1", LightBank::SideOne, item));
        assert!(profiles.modify_light("1", LightBank::SideOne, 3, LightItem { speed: 4, ..item }));
        assert!(
            profiles
                .lights("1", LightBank::Keyboard)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            profiles.lights("1", LightBank::SideOne).unwrap()[0].speed,
            4
        );
        assert!(profiles.set_light_using_index("1", LightBank::SideOne, 3));
        assert_eq!(profiles.light_using_index("1", LightBank::SideOne), Some(3));
    }

    #[test]
    fn new_documents_match_original_dll_fixtures() {
        fn normalize(element: &mut Element) {
            element
                .children
                .retain(|child| !matches!(child, Child::Text(text) if text.trim().is_empty()));
            element.elements_mut().for_each(normalize);
        }

        let mut profiles = Profiles::default();
        profiles.append_profile("3", "Alice & Bob", true);
        profiles.append_profile("1", "Default", false);
        profiles.rename_profile("1", "Renamed");
        let mut reference = Profiles::from_xml(include_bytes!(
            "../../tests/fixtures/vendor-sdk/parameters.xml"
        ))
        .unwrap();
        normalize(&mut reference.root);
        assert_eq!(profiles.root, reference.root);

        let mut reference = Profiles::from_xml(include_bytes!(
            "../../tests/fixtures/vendor-sdk/advanced.xml"
        ))
        .unwrap();
        normalize(&mut reference.root);
        assert_eq!(Profiles::advanced_document().root, reference.root);
    }
}
#[test]
fn rejects_dtds_but_accepts_standard_escapes() {
    assert!(
        Profiles::from_xml(
            br#"<!DOCTYPE Parameters [<!ENTITY name "preset">]>
            <Parameters><config index="1" name="&name;"/></Parameters>"#
        )
        .is_err()
    );
    let profiles =
        Profiles::from_xml(br#"<Parameters><config index="1" name="A &amp; B"/></Parameters>"#)
            .unwrap();
    assert_eq!(profiles.profile("1").unwrap().name, "A & B");
}

#[test]
fn refuses_to_serialize_invalid_xml_characters() {
    let mut profiles = Profiles::default();
    profiles.append_profile("1", "invalid\0name", false);
    assert!(profiles.to_xml().is_err());
}

#[test]
fn bounds_total_nodes_even_with_a_small_input_file() {
    let xml = format!(
        "<Parameters>{}</Parameters>",
        "<item/>".repeat(MAX_XML_NODES)
    );
    assert!(xml.len() < MAX_XML_BYTES);
    let Err(error) = Profiles::from_xml(xml.as_bytes()) else {
        panic!("profile exceeded the node limit");
    };
    assert_eq!(error.to_string(), "profile XML exceeds the node limit");
}
