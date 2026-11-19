use super::{Child, Element, MacroEvent, Profiles, indexed, remove_indexed};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MacroInfo<'a> {
    pub index: &'a str,
    pub name: &'a str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MacroUpdate<'a> {
    pub delay: &'a str,
    pub event_type: &'a str,
    pub key_code: &'a str,
    pub code_name: &'a str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Occurrence {
    First,
    Second,
}

impl Occurrence {
    fn index(self) -> usize {
        match self {
            Self::First => 0,
            Self::Second => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MacroDirection {
    Up,
    Down,
}

impl Profiles {
    pub fn macro_info(&self, index: &str) -> Option<MacroInfo<'_>> {
        let node = self
            .root
            .elements()
            .find(|node| indexed(node, "macro", index))?;
        Some(MacroInfo {
            index: node.attribute("index")?,
            name: node.attribute("name").unwrap_or_default(),
        })
    }

    pub fn list_macros(&self) -> Vec<MacroInfo<'_>> {
        self.root
            .elements()
            .filter(|node| node.name == "macro")
            .map(|node| MacroInfo {
                index: node.attribute("index").unwrap_or_default(),
                name: node.attribute("name").unwrap_or_default(),
            })
            .collect()
    }

    pub fn rename_macro(&mut self, index: &str, name: &str) -> bool {
        let Some(node) = self.macro_mut(index) else {
            return false;
        };
        node.set("name", name);
        true
    }

    pub fn remove_macro(&mut self, index: &str) -> bool {
        remove_indexed(&mut self.root, "macro", index)
    }

    pub fn remove_macro_events(&mut self, index: &str, mark: &str) -> usize {
        let Some(node) = self.macro_mut(index) else {
            return 0;
        };
        let count = node.children.len();
        node.children.retain(|child| !marked(child, mark));
        count - node.children.len()
    }

    pub fn update_macro_event(
        &mut self,
        index: &str,
        mark: &str,
        which: Occurrence,
        update: MacroUpdate<'_>,
    ) -> bool {
        let Some(node) = self.macro_mut(index) else {
            return false;
        };
        let Some(item) = node
            .elements_mut()
            .filter(|item| item.name == "item" && item.attribute("mark") == Some(mark))
            .nth(which.index())
        else {
            return false;
        };
        for (name, value) in [
            ("delay", update.delay),
            ("type", update.event_type),
            ("keyCode", update.key_code),
            ("codeName", update.code_name),
        ] {
            item.set(name, value);
        }
        true
    }

    pub fn move_macro_event(
        &mut self,
        index: &str,
        mark: &str,
        which: Occurrence,
        direction: MacroDirection,
    ) -> bool {
        let Some(node) = self.macro_mut(index) else {
            return false;
        };
        let Some(position) = node
            .children
            .iter()
            .enumerate()
            .filter_map(|(i, child)| marked(child, mark).then_some(i))
            .nth(which.index())
        else {
            return false;
        };
        let target = match direction {
            MacroDirection::Up => node.children[..position].iter().rposition(is_element),
            MacroDirection::Down => node.children[position + 1..]
                .iter()
                .position(is_element)
                .map(|i| position + 1 + i),
        };
        let Some(target) = target else { return false };
        let event = node.children.remove(position);
        node.children.insert(target, event);
        true
    }

    pub fn append_bound_macro(
        &mut self,
        profile: &str,
        info: MacroInfo<'_>,
        items: &[MacroEvent<'_>],
    ) -> bool {
        let Some(bindings) = self
            .config_mut(profile)
            .and_then(|node| node.child_mut("bindMacro"))
        else {
            return false;
        };
        let mut node = Element::new("macro");
        node.set("index", info.index);
        node.set("name", info.name);
        append_events(&mut node, items);
        bindings.append(node);
        true
    }

    pub fn bound_macro_events(&self, profile: &str, index: &str) -> Option<Vec<MacroEvent<'_>>> {
        let node = self
            .config(profile)?
            .child("bindMacro")?
            .elements()
            .find(|node| indexed(node, "macro", index))?;
        Some(events(node))
    }

    pub fn remove_bound_macro(&mut self, profile: &str, index: &str) -> bool {
        self.config_mut(profile)
            .and_then(|node| node.child_mut("bindMacro"))
            .is_some_and(|bindings| remove_indexed(bindings, "macro", index))
    }

    fn macro_mut(&mut self, index: &str) -> Option<&mut Element> {
        self.root
            .elements_mut()
            .find(|node| indexed(node, "macro", index))
    }
}

fn marked(node: &Child, mark: &str) -> bool {
    matches!(node, Child::Element(item) if item.name == "item" && item.attribute("mark") == Some(mark))
}

fn is_element(node: &Child) -> bool {
    matches!(node, Child::Element(_))
}

pub(super) fn events(node: &Element) -> Vec<MacroEvent<'_>> {
    node.elements()
        .filter(|item| item.name == "item")
        .map(|item| {
            let attr = |name| item.attribute(name).unwrap_or_default();
            MacroEvent {
                mark: attr("mark"),
                delay: attr("delay"),
                event_type: attr("type"),
                key_code: attr("keyCode"),
                code_name: attr("codeName"),
                explanation: attr("operateExplain"),
            }
        })
        .collect()
}

pub(super) fn append_events(node: &mut Element, events: &[MacroEvent<'_>]) {
    for event in events {
        let mut item = Element::new("item");
        for (key, value) in [
            ("mark", event.mark),
            ("delay", event.delay),
            ("type", event.event_type),
            ("keyCode", event.key_code),
            ("codeName", event.code_name),
            ("operateExplain", event.explanation),
        ] {
            item.set(key, value);
        }
        node.append(item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updates_and_moves_the_selected_event_in_a_pair() {
        let mut doc = Profiles::from_xml(
            br#"<Parameters><macro index="1">
            <item mark="pair" operateExplain="down" />
            <item mark="pair" operateExplain="up" />
            <item mark="other" />
            </macro></Parameters>"#,
        )
        .unwrap();
        let update = MacroUpdate {
            delay: "30",
            event_type: "key",
            key_code: "5",
            code_name: "B",
        };
        assert!(doc.update_macro_event("1", "pair", Occurrence::Second, update));
        assert!(doc.move_macro_event("1", "pair", Occurrence::Second, MacroDirection::Down));
        let events = doc.macro_events("1").unwrap();
        assert_eq!(
            events.iter().map(|e| e.mark).collect::<Vec<_>>(),
            ["pair", "other", "pair"]
        );
        assert_eq!(events[2].delay, "30");
        assert_eq!(events[2].explanation, "up");
        assert_eq!(doc.remove_macro_events("1", "pair"), 2);
        assert_eq!(doc.macro_events("1").unwrap()[0].mark, "other");
    }
}
