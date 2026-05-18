use anyhow::Result;
use serde_json::{Value, json};

pub fn build_b3dm(glb: &[u8], batch_length: usize, batch_table_json: &Value) -> Result<Vec<u8>> {
    let mut feature_json = serde_json::to_vec(&json!({ "BATCH_LENGTH": batch_length }))?;
    pad_to_alignment_at_offset(&mut feature_json, 28, 8, b' ');

    let mut batch_json = serde_json::to_vec(batch_table_json)?;
    pad_to_alignment_at_offset(&mut batch_json, 28 + feature_json.len(), 8, b' ');

    let byte_length = 28 + feature_json.len() + batch_json.len() + glb.len();
    let mut out = Vec::with_capacity(byte_length);
    out.extend_from_slice(b"b3dm");
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&(byte_length as u32).to_le_bytes());
    out.extend_from_slice(&(feature_json.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(batch_json.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&feature_json);
    out.extend_from_slice(&batch_json);
    out.extend_from_slice(glb);
    Ok(out)
}

fn pad_to_alignment_at_offset(bytes: &mut Vec<u8>, start_offset: usize, alignment: usize, pad: u8) {
    while !(start_offset + bytes.len()).is_multiple_of(alignment) {
        bytes.push(pad);
    }
}
