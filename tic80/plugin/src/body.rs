use glassvm_core::{BodyAction, BodyRuntime, FrameStartActionSequence, PortStimulusRequest};
use serde_json::{Value, json};

#[derive(Debug, Default)]
pub(super) struct Tic80BodyRuntime {
    sequence: FrameStartActionSequence,
    controller_mask: u32,
}

impl Tic80BodyRuntime {
    fn exact_object<'a>(
        value: &'a Value,
        action: &str,
        keys: &[&str],
    ) -> Result<&'a serde_json::Map<String, Value>, String> {
        let object = value
            .as_object()
            .ok_or_else(|| format!("TIC-80 body action {action:?} requires an object"))?;
        if object.len() != keys.len() || keys.iter().any(|key| !object.contains_key(*key)) {
            return Err(format!(
                "TIC-80 body action {action:?} requires exactly fields {keys:?}"
            ));
        }
        Ok(object)
    }

    fn button_bit(value: &Value, action: &str) -> Result<u32, String> {
        let object = Self::exact_object(value, action, &["player", "button"])?;
        let player = object["player"]
            .as_u64()
            .and_then(|player| u32::try_from(player).ok())
            .filter(|player| *player < 4)
            .ok_or_else(|| {
                format!("TIC-80 body action {action:?} player must be an integer in 0..=3")
            })?;
        let button = object["button"]
            .as_str()
            .ok_or_else(|| format!("TIC-80 body action {action:?} button must be a string"))?;
        let button_index = match button {
            "up" => 0,
            "down" => 1,
            "left" => 2,
            "right" => 3,
            "a" => 4,
            "b" => 5,
            "x" => 6,
            "y" => 7,
            other => {
                return Err(format!(
                    "TIC-80 body action {action:?} has unsupported button {other:?}"
                ));
            }
        };
        Ok(1_u32 << (player * 8 + button_index))
    }
}

impl BodyRuntime for Tic80BodyRuntime {
    fn resolve_action(&mut self, action: &BodyAction) -> Result<Vec<PortStimulusRequest>, String> {
        let next_mask = match action.action.as_str() {
            "set_controller_mask" => {
                let object = Self::exact_object(&action.value, &action.action, &["mask"])?;
                object["mask"]
                    .as_u64()
                    .and_then(|mask| u32::try_from(mask).ok())
                    .ok_or_else(|| {
                        "TIC-80 body action \"set_controller_mask\" requires a 32-bit unsigned mask"
                            .to_string()
                    })?
            }
            "press_button" => {
                self.controller_mask | Self::button_bit(&action.value, &action.action)?
            }
            "release_button" => {
                self.controller_mask & !Self::button_bit(&action.value, &action.action)?
            }
            "release_all" => {
                if action.value != Value::Null && action.value != json!({}) {
                    return Err("TIC-80 body action \"release_all\" accepts only null or {}".into());
                }
                0
            }
            other => return Err(format!("unsupported TIC-80 native body action {other:?}")),
        };
        self.sequence.accept("TIC-80", action)?;
        self.controller_mask = next_mask;

        Ok(vec![PortStimulusRequest {
            coordinate: action.coordinate.clone(),
            port: "gamepad_in".into(),
            value: json!({"mask": self.controller_mask}),
        }])
    }
}
