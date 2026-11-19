use super::{Child, Element, Profiles, decimal};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PosEvent {
    pub mark: i32,
    pub function_index: i32,
    pub event_type: i32,
    pub key_code: i32,
}

const FIELDS: [&str; 4] = ["mark", "fnIndex", "type", "keyCode"];

impl Profiles {
    pub fn append_pos_event_group(&mut self, index: &str, name: &str) {
        self.append_named_group("event", index, name);
    }

    pub fn append_pos_layer(&mut self, index: &str, name: &str) {
        self.append_named_group("layer", index, name);
    }

    pub fn append_pos_event(&mut self, group_index: i32, event: PosEvent) -> bool {
        let Some(group) = self
            .root
            .elements_mut()
            .find(|node| event_group(node, group_index))
        else {
            return false;
        };
        let mut item = Element::new("item");
        for (name, value) in FIELDS.into_iter().zip([
            event.mark,
            event.function_index,
            event.event_type,
            event.key_code,
        ]) {
            item.set(name, value.to_string());
        }
        group.append(item);
        true
    }

    pub fn pos_events(&self, group_index: i32) -> Option<impl Iterator<Item = PosEvent> + '_> {
        let group = self
            .root
            .elements()
            .find(|node| event_group(node, group_index))?;
        Some(
            group
                .elements()
                .filter(|node| node.name == "item")
                .map(read_event),
        )
    }

    pub fn pos_events_for_function(
        &self,
        function_index: i32,
    ) -> Option<impl Iterator<Item = (usize, PosEvent)> + '_> {
        let group = self.root.child("event")?;
        Some(group.elements().enumerate().filter_map(move |(i, node)| {
            if node.name != "item" || node.attribute("fnIndex").map(decimal) != Some(function_index)
            {
                return None;
            }
            Some((i + 1, read_event(node)))
        }))
    }

    pub fn remove_pos_events(&mut self, mark: i32) -> usize {
        let Some(group) = self.root.child_mut("event") else {
            return 0;
        };
        let count = group.children.len();
        group.children.retain(|child| {
            !matches!(child, Child::Element(node) if
                node.name == "item" && node.attribute("mark").map(decimal) == Some(mark))
        });
        count - group.children.len()
    }

    fn append_named_group(&mut self, element: &str, index: &str, name: &str) {
        let mut group = Element::new(element);
        group.set("index", index);
        group.set("name", name);
        self.root.append(group);
    }
}

fn event_group(node: &Element, index: i32) -> bool {
    node.name == "event" && node.attribute("index").map(decimal) == Some(index)
}

fn read_event(node: &Element) -> PosEvent {
    let [mark, function_index, event_type, key_code] =
        FIELDS.map(|name| node.attribute(name).map_or(0, decimal));
    PosEvent {
        mark,
        function_index,
        event_type,
        key_code,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_a_function_and_preserves_document_positions() {
        let mut doc = Profiles::default();
        doc.append_pos_event_group("1", "events");
        doc.append_pos_layer("2", "layer");
        let item = PosEvent {
            mark: 1,
            function_index: 3,
            event_type: 4,
            key_code: 5,
        };
        assert!(doc.append_pos_event(1, item));
        assert!(doc.append_pos_event(
            1,
            PosEvent {
                mark: 2,
                function_index: 6,
                ..item
            }
        ));
        let selected = doc.pos_events_for_function(6).unwrap().collect::<Vec<_>>();
        assert_eq!(
            selected[0],
            (
                2,
                PosEvent {
                    mark: 2,
                    function_index: 6,
                    ..item
                }
            )
        );
        assert_eq!(doc.remove_pos_events(1), 1);
        assert_eq!(doc.pos_events(1).unwrap().count(), 1);
    }
}
