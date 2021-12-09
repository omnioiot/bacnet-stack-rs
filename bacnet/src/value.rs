/// Values that are returned from reading properties.
///
#[derive(Debug)]
pub enum BACnetValue {
    Null, // Yes!
    Bool(bool),
    Uint(u64),
    Int(i32),
    Real(f32),
    Double(f64),
    String(String),            // BACNET_CHARACTER_STRING
    Bytes(Vec<u8>),            // BACNET_OCTET_STRING
    Enum(u32, Option<String>), // Enumerated values also have string representations...
}
