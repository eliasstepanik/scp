use serde_json::Value;

/// Strips fields at dot-separated paths from a JSON Value in-place.
/// Silently ignores paths that don't exist.
/// Example path: "metadata.managedFields" removes obj["metadata"]["managedFields"]
pub fn strip_fields(value: &mut Value, paths: &[String]) {
    for path in paths {
        let parts: Vec<&str> = path.split('.').collect();
        strip_path(value, &parts);
    }
}

fn strip_path(value: &mut Value, parts: &[&str]) {
    if parts.is_empty() {
        return;
    }
    match value {
        Value::Object(map) => {
            if parts.len() == 1 {
                map.remove(parts[0]);
            } else if let Some(child) = map.get_mut(parts[0]) {
                strip_path(child, &parts[1..]);
            }
        }
        Value::Array(arr) => {
            // Apply recursively to every array element
            for item in arr.iter_mut() {
                strip_path(item, parts);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_strip_top_level_field() {
        let mut val = json!({"a": 1, "b": 2});
        strip_fields(&mut val, &["a".to_string()]);
        assert_eq!(val, json!({"b": 2}));
    }

    #[test]
    fn test_strip_nested_field() {
        let mut val = json!({"metadata": {"managedFields": ["x"], "name": "foo"}, "spec": {}});
        strip_fields(&mut val, &["metadata.managedFields".to_string()]);
        assert_eq!(val, json!({"metadata": {"name": "foo"}, "spec": {}}));
    }

    #[test]
    fn test_strip_nonexistent_path_is_noop() {
        let mut val = json!({"a": 1});
        strip_fields(&mut val, &["b.c.d".to_string()]);
        assert_eq!(val, json!({"a": 1}));
    }

    #[test]
    fn test_strip_applies_to_array_elements() {
        let mut val = json!([
            {"metadata": {"managedFields": ["x"], "name": "foo"}},
            {"metadata": {"managedFields": ["y"], "name": "bar"}}
        ]);
        strip_fields(&mut val, &["metadata.managedFields".to_string()]);
        assert_eq!(
            val,
            json!([
                {"metadata": {"name": "foo"}},
                {"metadata": {"name": "bar"}}
            ])
        );
    }

    #[test]
    fn test_strip_multiple_paths() {
        let mut val = json!({"metadata": {"managedFields": ["x"], "annotations": {"k": "v"}, "name": "foo"}});
        strip_fields(
            &mut val,
            &[
                "metadata.managedFields".to_string(),
                "metadata.annotations".to_string(),
            ],
        );
        assert_eq!(val, json!({"metadata": {"name": "foo"}}));
    }

    #[test]
    fn test_strip_empty_paths_is_noop() {
        let mut val = json!({"a": 1});
        strip_fields(&mut val, &[]);
        assert_eq!(val, json!({"a": 1}));
    }
}
