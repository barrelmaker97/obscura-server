use obscura_server::proto::obscura::v1::{EncryptedMessage, encrypted_message::Type};
use prost::Message;

fn main() {
    let msg = EncryptedMessage {
        r#type: Type::TypeEncryptedMessage as i32,
        content: b"test content".to_vec(),
    };
    
    let mut buf = Vec::new();
    msg.encode(&mut buf).unwrap();
    
    let decoded = EncryptedMessage::decode(&buf[..]).unwrap();
    println!("Decoded type: {:?}", decoded.type);
    assert_eq!(decoded.type, Type::TypeEncryptedMessage as i32);
    println!("Successfully encoded and decoded message with new enum variants");
}
