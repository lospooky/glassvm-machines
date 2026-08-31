use glassvm_core::{BodyAction, BodyRuntime, FrameStartActionSequence, PortStimulusRequest};
use serde_json::{Value, json};

#[derive(Default)]
pub(super) struct Pico8HeadlessBodyRuntime {
    sequence: FrameStartActionSequence,
    controller_mask: u16,
}

impl Pico8HeadlessBodyRuntime {
    fn exact_mask(value: &Value, action: &str) -> Result<u16, String> {
        let object = value
            .as_object()
            .ok_or_else(|| format!("PICO-8 body action {action:?} requires an object"))?;
        if object.len() != 1 || !object.contains_key("mask") {
            return Err(format!(
                "PICO-8 body action {action:?} requires exactly {{\"mask\": <u16>}}"
            ));
        }
        let mask = object["mask"]
            .as_u64()
            .ok_or_else(|| format!("PICO-8 body action {action:?} requires an unsigned mask"))?;
        u16::try_from(mask).map_err(|_| format!("PICO-8 controller mask {mask:#x} exceeds 16 bits"))
    }

    fn exact_button(value: &Value, action: &str) -> Result<u32, String> {
        let object = value
            .as_object()
            .ok_or_else(|| format!("PICO-8 body action {action:?} requires an object"))?;
        if object.len() != 2 || !object.contains_key("player") || !object.contains_key("button") {
            return Err(format!(
                "PICO-8 body action {action:?} requires exactly {{\"player\": 0|1, \"button\": <name>}}"
            ));
        }
        let player = object["player"]
            .as_u64()
            .filter(|player| *player < 2)
            .ok_or_else(|| format!("PICO-8 body action {action:?} player must be 0 or 1"))?;
        let button = object["button"]
            .as_str()
            .ok_or_else(|| format!("PICO-8 body action {action:?} button must be a string"))?;
        let button = match button {
            "left" => 0,
            "right" => 1,
            "up" => 2,
            "down" => 3,
            "a" => 4,
            "b" => 5,
            other => {
                return Err(format!(
                    "unsupported PICO-8 button {other:?}; expected left, right, up, down, a, or b"
                ));
            }
        };
        Ok((player as u32) * 8 + button)
    }

    fn release_all(value: &Value) -> Result<(), String> {
        if value == &Value::Null || value.as_object().is_some_and(serde_json::Map::is_empty) {
            Ok(())
        } else {
            Err("PICO-8 body action \"release_all\" accepts only null or {}".into())
        }
    }
}

impl BodyRuntime for Pico8HeadlessBodyRuntime {
    fn resolve_action(&mut self, action: &BodyAction) -> Result<Vec<PortStimulusRequest>, String> {
        let next_mask = match action.action.as_str() {
            "set_controller_mask" => Self::exact_mask(&action.value, &action.action)?,
            "press_button" => {
                self.controller_mask | (1_u16 << Self::exact_button(&action.value, &action.action)?)
            }
            "release_button" => {
                self.controller_mask
                    & !(1_u16 << Self::exact_button(&action.value, &action.action)?)
            }
            "release_all" => {
                Self::release_all(&action.value)?;
                0
            }
            other => return Err(format!("unsupported PICO-8 headless body action {other:?}")),
        };
        self.sequence.accept("PICO-8", action)?;
        self.controller_mask = next_mask;
        Ok(vec![PortStimulusRequest {
            coordinate: action.coordinate.clone(),
            port: "controllers_in".into(),
            value: json!({"mask": self.controller_mask}),
        }])
    }
}
