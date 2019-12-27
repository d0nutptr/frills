use serde::{Deserialize, Serialize};

#[test]
fn test_nested_serialization_deserializationn() {
    let data = ExampleNestedStructure {
        version: 123,
        value: InternalValue {
            value: r#"{"msg": "Some Fake Data"}"#.to_string()
        }
    };

    let result = match bincode::serialize(&data) {
        Ok(result) => result,
        Err(e) => {
            assert!(false, e);
            return;
        }
    };

    let restructured: ExampleNestedStructure = match bincode::deserialize(&result) {
        Ok(output) => output,
        Err(e) => {
            assert!(false, e);
            return;
        }
    };

    assert_eq!(restructured.version, data.version);
    assert_eq!(restructured.value.value, data.value.value);
}

#[derive(Serialize, Deserialize)]
struct ExampleNestedStructure {
    pub version: u32,
    pub value: InternalValue
}

#[derive(Serialize, Deserialize)]
struct InternalValue {
    pub value: String
}