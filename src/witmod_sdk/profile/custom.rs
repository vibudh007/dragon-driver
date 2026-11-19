use super::{Child, Element, Profiles, decimal, indexed, remove_indexed};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CustomLightInfo<'a> {
    pub index: &'a str,
    pub name: &'a str,
    pub is_using: &'a str,
    pub flag_index: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CustomLightKey<'a> {
    pub key_code: &'a str,
    pub code_name: &'a str,
    pub index_x: i32,
    pub index_y: i32,
    pub color: [i32; 3],
    pub transparency: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyAction<'a> {
    pub key_code: &'a str,
    pub key_character: &'a str,
    pub operation_type: &'a str,
    pub repeat_count: &'a str,
    pub mode: &'a str,
    pub value: &'a str,
}

const COLOR_FIELDS: [&str; 3] = ["colorR", "colorG", "colorB"];

impl Profiles {
    pub fn custom_lights(&self, profile: &str) -> Option<Vec<CustomLightInfo<'_>>> {
        let container = self.config(profile)?.child("customLight")?;
        Some(
            container
                .elements()
                .filter(|node| node.name == "customLed")
                .map(|node| CustomLightInfo {
                    index: attr(node, "index"),
                    name: attr(node, "name"),
                    is_using: attr(node, "isUsing"),
                    flag_index: number(node, "usingFlagIndex"),
                })
                .collect(),
        )
    }

    pub fn append_custom_light(&mut self, profile: &str, info: CustomLightInfo<'_>) -> bool {
        let Some(container) = self
            .config_mut(profile)
            .and_then(|node| node.child_mut("customLight"))
        else {
            return false;
        };
        let mut node = Element::new("customLed");
        node.set("index", info.index);
        node.set("name", info.name);
        node.set("isUsing", info.is_using);
        node.set("usingFlagIndex", info.flag_index.to_string());
        container.append(node);
        true
    }

    pub fn rename_custom_light(&mut self, profile: &str, index: &str, name: &str) -> bool {
        let Some(node) = self.custom_light_mut(profile, index) else {
            return false;
        };
        node.set("name", name);
        true
    }

    pub fn set_custom_light_usage(
        &mut self,
        profile: &str,
        index: &str,
        is_using: &str,
        flag_index: i32,
    ) -> bool {
        let Some(node) = self.custom_light_mut(profile, index) else {
            return false;
        };
        node.set("isUsing", is_using);
        node.set("usingFlagIndex", flag_index.to_string());
        true
    }

    pub fn remove_custom_light(&mut self, profile: &str, index: &str) -> bool {
        let Some(container) = self
            .config_mut(profile)
            .and_then(|node| node.child_mut("customLight"))
        else {
            return false;
        };
        remove_indexed(container, "customLed", index)
    }

    pub fn clear_custom_light(&mut self, profile: &str, index: &str) -> bool {
        let Some(node) = self.custom_light_mut(profile, index) else {
            return false;
        };
        node.children.clear();
        true
    }

    pub fn contains_custom_light(&self, profile: &str, index: &str) -> bool {
        self.config(profile)
            .and_then(|node| node.child("customLight"))
            .is_some_and(|node| {
                node.elements()
                    .any(|node| indexed(node, "customLed", index))
            })
    }

    pub fn first_reserved_custom_light_index(&self, profile: &str) -> i32 {
        let mut selected = -1;
        let Some(container) = self
            .config(profile)
            .and_then(|node| node.child("customLight"))
        else {
            return selected;
        };
        for node in container.elements() {
            for (name, value) in &node.attributes {
                match name.as_str() {
                    "index" => selected = decimal(value),
                    "usingFlagIndex" if matches!(decimal(value), 0 | 1) => return selected,
                    _ => {}
                }
            }
        }
        -1
    }

    pub fn custom_light_keys(&self, profile: &str, index: &str) -> Option<Vec<CustomLightKey<'_>>> {
        let node = self
            .config(profile)?
            .child("customLight")?
            .elements()
            .find(|node| indexed(node, "customLed", index))?;
        Some(
            node.elements()
                .filter(|item| item.name == "item")
                .map(light_key)
                .collect(),
        )
    }

    pub fn standalone_custom_light_keys(&self, index: &str) -> Option<Vec<CustomLightKey<'_>>> {
        let node = self
            .root
            .elements()
            .find(|node| node.attribute("index") == Some(index))?;
        Some(node.elements().map(light_key).collect())
    }

    pub fn custom_light_keys_for_flag(
        &self,
        profile: &str,
        flag: &str,
    ) -> Option<Vec<CustomLightKey<'_>>> {
        let parent = self.config(profile)?.child("customLight")?;
        let node = parent
            .elements()
            .find(|node| node.attribute("usingFlagIndex") == Some(flag))?;
        Some(node.elements().map(light_key).collect())
    }

    pub fn append_custom_light_key(
        &mut self,
        profile: &str,
        index: &str,
        key: CustomLightKey<'_>,
    ) -> bool {
        let Some(node) = self.custom_light_mut(profile, index) else {
            return false;
        };
        let mut item = Element::new("item");
        item.set("keyCode", key.key_code);
        item.set("codeName", key.code_name);
        item.set("indexX", key.index_x.to_string());
        item.set("indexY", key.index_y.to_string());
        for (channel, value) in COLOR_FIELDS.into_iter().zip(key.color) {
            item.set(channel, value.to_string());
        }
        item.set("transparentNum", key.transparency.to_string());
        node.append(item);
        true
    }

    pub fn key_actions(&self, profile: &str) -> Option<impl Iterator<Item = KeyAction<'_>>> {
        let node = self.config(profile)?.child("keyEvent")?;
        Some(
            node.elements()
                .filter(|item| item.name == "item")
                .map(|item| KeyAction {
                    key_code: attr(item, "keyCode"),
                    key_character: attr(item, "keyChar"),
                    operation_type: attr(item, "operateType"),
                    repeat_count: attr(item, "repeatNum"),
                    mode: attr(item, "mode"),
                    value: attr(item, "Value"),
                }),
        )
    }

    pub fn assigned_key_codes(&self, profile: &str) -> Option<impl Iterator<Item = &str>> {
        let container = self.config(profile)?.child("keyEvent")?;
        Some(
            container
                .elements()
                .filter_map(|item| item.attribute("keyCode")),
        )
    }

    pub fn has_key_action(&self, profile: &str, code: &str) -> bool {
        self.assigned_key_codes(profile)
            .is_some_and(|mut codes| codes.any(|key| key == code))
    }

    pub fn append_key_action(&mut self, profile: &str, action: KeyAction<'_>) -> bool {
        let Some(node) = self
            .config_mut(profile)
            .and_then(|node| node.child_mut("keyEvent"))
        else {
            return false;
        };
        let mut item = Element::new("item");
        for (name, value) in [
            ("keyCode", action.key_code),
            ("keyChar", action.key_character),
            ("operateType", action.operation_type),
            ("repeatNum", action.repeat_count),
            ("mode", action.mode),
            ("Value", action.value),
        ] {
            item.set(name, value);
        }
        node.append(item);
        true
    }

    pub fn remap_switch_targets(
        &mut self,
        profile: &str,
        targets: &std::collections::HashMap<String, String>,
    ) -> bool {
        let Some(events) = self
            .config_mut(profile)
            .and_then(|node| node.child_mut("keyEvent"))
        else {
            return true;
        };
        let mut replacements = Vec::new();
        for (index, child) in events.children.iter().enumerate() {
            let Child::Element(event) = child else {
                continue;
            };
            if event.name != "item" || event.attribute("operateType") != Some("7") {
                continue;
            }
            let Some(new) = event.attribute("Value").and_then(|id| targets.get(id)) else {
                return false;
            };
            replacements.push((index, new));
        }
        for (index, target) in replacements {
            if let Child::Element(event) = &mut events.children[index] {
                event.set("Value", target);
            }
        }
        true
    }

    pub fn remove_key_action(&mut self, profile: &str, code: &str) -> bool {
        let Some(node) = self
            .config_mut(profile)
            .and_then(|node| node.child_mut("keyEvent"))
        else {
            return false;
        };
        let Some(index) = node.children.iter().position(|child| {
            matches!(child, Child::Element(item) if item.name == "item" && item.attribute("keyCode") == Some(code))
        }) else { return false };
        node.children.remove(index);
        true
    }

    pub fn count_key_action_values(
        &self,
        profile: &str,
        operate_type: &str,
        value: &str,
    ) -> Option<usize> {
        Some(
            self.key_actions(profile)?
                .filter(|item| item.operation_type == operate_type && item.value == value)
                .count(),
        )
    }

    fn custom_light_mut(&mut self, profile: &str, index: &str) -> Option<&mut Element> {
        self.config_mut(profile)?
            .child_mut("customLight")?
            .elements_mut()
            .find(|node| indexed(node, "customLed", index))
    }
}

fn light_key(item: &Element) -> CustomLightKey<'_> {
    CustomLightKey {
        key_code: attr(item, "keyCode"),
        code_name: attr(item, "codeName"),
        index_x: number(item, "indexX"),
        index_y: number(item, "indexY"),
        color: COLOR_FIELDS.map(|channel| number(item, channel)),
        transparency: number(item, "transparentNum"),
    }
}

fn attr<'a>(item: &'a Element, name: &str) -> &'a str {
    item.attribute(name).unwrap_or_default()
}

fn number(item: &Element, name: &str) -> i32 {
    item.attribute(name).map_or(0, decimal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_custom_light_metadata_and_keys_independently() {
        let mut profiles = Profiles::default();
        profiles.append_profile("1", "test", false);
        let info = CustomLightInfo {
            index: "3",
            name: "colors",
            is_using: "0",
            flag_index: 0,
        };
        let key = CustomLightKey {
            key_code: "4",
            code_name: "A",
            index_x: 1,
            index_y: 2,
            color: [255, 100, 0],
            transparency: 80,
        };
        assert!(profiles.append_custom_light("1", info));
        assert!(profiles.append_custom_light_key("1", "3", key));
        assert!(profiles.set_custom_light_usage("1", "3", "1", 7));
        assert_eq!(profiles.custom_light_keys("1", "3").unwrap(), vec![key]);
        assert!(profiles.clear_custom_light("1", "3"));
        assert_eq!(profiles.custom_lights("1").unwrap()[0].flag_index, 7);
        assert!(profiles.custom_light_keys("1", "3").unwrap().is_empty());
    }

    #[test]
    fn counts_actions_using_both_operation_and_value() {
        let mut profiles = Profiles::default();
        profiles.append_profile("1", "test", false);
        let action = KeyAction {
            key_code: "4",
            key_character: "A",
            operation_type: "2",
            repeat_count: "1",
            mode: "0",
            value: "1",
        };
        assert!(profiles.append_key_action("1", action));
        assert!(profiles.append_key_action(
            "1",
            KeyAction {
                key_code: "5",
                operation_type: "3",
                ..action
            }
        ));
        assert_eq!(profiles.count_key_action_values("1", "2", "1"), Some(1));
        assert!(profiles.remove_key_action("1", "4"));
        assert_eq!(profiles.count_key_action_values("1", "2", "1"), Some(0));
    }
}
