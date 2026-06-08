use serde_json::Value;

pub fn deep_merge_settings(base: &Value, overrides: &Value) -> Value {
    match (base, overrides) {
        (Value::Object(base), Value::Object(overrides)) => {
            let mut result = base.clone();
            for (key, override_value) in overrides {
                if override_value.is_null() {
                    continue;
                }
                let next_value = if let Some(base_value) = result.get(key) {
                    if base_value.is_object() && override_value.is_object() {
                        deep_merge_settings(base_value, override_value)
                    } else {
                        override_value.clone()
                    }
                } else {
                    override_value.clone()
                };
                result.insert(key.clone(), next_value);
            }
            Value::Object(result)
        }
        _ => overrides.clone(),
    }
}
