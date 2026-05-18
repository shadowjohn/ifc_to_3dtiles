use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct EntityRecord {
    pub id: u32,
    pub type_name: String,
    body_start: usize,
    body_end: usize,
    stmt_start: usize,
    stmt_end: usize,
}

#[derive(Debug, Clone)]
pub struct StepIndex {
    content: String,
    entities: Vec<EntityRecord>,
    by_id: HashMap<u32, usize>,
}

impl StepIndex {
    pub fn parse(content: impl Into<String>) -> Self {
        let content = content.into();
        let bytes = content.as_bytes();
        let mut entities = Vec::new();
        let mut by_id = HashMap::new();
        let mut i = 0;

        while i < bytes.len() {
            if bytes[i] != b'#' || i + 1 >= bytes.len() || !bytes[i + 1].is_ascii_digit() {
                i += 1;
                continue;
            }

            let stmt_start = i;
            let mut j = i + 1;
            let mut id: u32 = 0;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                id = id
                    .saturating_mul(10)
                    .saturating_add((bytes[j] - b'0') as u32);
                j += 1;
            }
            j = skip_ws(bytes, j);
            if j >= bytes.len() || bytes[j] != b'=' {
                i += 1;
                continue;
            }
            j += 1;
            j = skip_ws(bytes, j);
            let type_start = j;
            while j < bytes.len()
                && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_' || bytes[j] == b'-')
            {
                j += 1;
            }
            if type_start == j {
                i += 1;
                continue;
            }
            let type_name = content[type_start..j].to_ascii_uppercase();
            j = skip_ws(bytes, j);
            if j >= bytes.len() || bytes[j] != b'(' {
                i += 1;
                continue;
            }
            let body_start = j + 1;
            let mut k = body_start;
            let mut depth = 1i32;
            let mut in_string = false;
            let mut found = None;

            while k < bytes.len() {
                let b = bytes[k];
                if in_string {
                    if b == b'\'' {
                        if k + 1 < bytes.len() && bytes[k + 1] == b'\'' {
                            k += 2;
                            continue;
                        }
                        in_string = false;
                    }
                    k += 1;
                    continue;
                }

                match b {
                    b'\'' => in_string = true,
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            let body_end = k;
                            k += 1;
                            while k < bytes.len() && bytes[k] != b';' {
                                k += 1;
                            }
                            if k < bytes.len() {
                                found = Some((body_end, k + 1));
                            }
                            break;
                        }
                    }
                    _ => {}
                }
                k += 1;
            }

            if let Some((body_end, stmt_end)) = found {
                let rec = EntityRecord {
                    id,
                    type_name,
                    body_start,
                    body_end,
                    stmt_start,
                    stmt_end,
                };
                by_id.insert(id, entities.len());
                entities.push(rec);
                i = stmt_end;
            } else {
                i += 1;
            }
        }

        Self {
            content,
            entities,
            by_id,
        }
    }

    pub fn entity(&self, id: u32) -> Option<&EntityRecord> {
        self.by_id.get(&id).and_then(|idx| self.entities.get(*idx))
    }

    pub fn body<'a>(&'a self, entity: &EntityRecord) -> &'a str {
        &self.content[entity.body_start..entity.body_end]
    }

    pub fn statement<'a>(&'a self, entity: &EntityRecord) -> &'a str {
        &self.content[entity.stmt_start..entity.stmt_end]
    }

    pub fn entities(&self) -> impl Iterator<Item = &EntityRecord> {
        self.entities.iter()
    }

    pub fn entities_by_type<'a>(
        &'a self,
        type_name: &'a str,
    ) -> impl Iterator<Item = &'a EntityRecord> + 'a {
        let wanted = type_name.to_ascii_uppercase();
        self.entities.iter().filter(move |e| e.type_name == wanted)
    }

    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
}

fn skip_ws(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

pub fn split_arguments(input: &str) -> Vec<&str> {
    let bytes = input.as_bytes();
    let mut args = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut depth = 0i32;
    let mut in_string = false;

    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            if b == b'\'' {
                if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                    i += 2;
                    continue;
                }
                in_string = false;
            }
            i += 1;
            continue;
        }

        match b {
            b'\'' => in_string = true,
            b'(' => depth += 1,
            b')' => depth -= 1,
            b',' if depth == 0 => {
                args.push(input[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    args.push(input[start..].trim());
    args
}

pub fn extract_first_ref(input: &str) -> Option<u32> {
    extract_refs(input).into_iter().next()
}

pub fn extract_refs(input: &str) -> Vec<u32> {
    let bytes = input.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'#' {
            let mut j = i + 1;
            let mut id = 0u32;
            let mut any = false;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                id = id
                    .saturating_mul(10)
                    .saturating_add((bytes[j] - b'0') as u32);
                any = true;
                j += 1;
            }
            if any {
                out.push(id);
                i = j;
                continue;
            }
        }
        i += 1;
    }
    out
}

pub fn decode_ifc_string(input: &str) -> String {
    let mut raw = input.trim();
    if raw == "$" || raw == "*" {
        return String::new();
    }
    if raw.starts_with('\'') && raw.ends_with('\'') && raw.len() >= 2 {
        raw = &raw[1..raw.len() - 1];
    }

    let mut out = String::new();
    let mut i = 0usize;
    while i < raw.len() {
        let rest = &raw[i..];
        if rest.starts_with("''") {
            out.push('\'');
            i += 2;
            continue;
        }
        if rest.starts_with("\\X2\\") {
            let start = i + 4;
            if let Some(end_rel) = raw[start..].find("\\X0\\") {
                let hex = &raw[start..start + end_rel];
                let mut units = Vec::new();
                for chunk in hex.as_bytes().chunks(4) {
                    if chunk.len() == 4
                        && let Ok(text) = std::str::from_utf8(chunk)
                        && let Ok(unit) = u16::from_str_radix(text, 16)
                    {
                        units.push(unit);
                    }
                }
                out.push_str(&String::from_utf16_lossy(&units));
                i = start + end_rel + 4;
                continue;
            }
        }
        if rest.starts_with("\\X\\") && i + 5 <= raw.len() {
            let hex = &raw[i + 3..i + 5];
            if let Ok(value) = u8::from_str_radix(hex, 16) {
                out.push(value as char);
                i += 5;
                continue;
            }
        }
        if let Some(ch) = rest.chars().next() {
            out.push(ch);
            i += ch.len_utf8();
        } else {
            break;
        }
    }
    out
}

pub fn numbers_in(input: &str) -> Vec<f64> {
    let bytes = input.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        let begins_number = b.is_ascii_digit()
            || b == b'.'
            || ((b == b'-' || b == b'+')
                && i + 1 < bytes.len()
                && (bytes[i + 1].is_ascii_digit() || bytes[i + 1] == b'.'));
        if !begins_number {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        while i < bytes.len()
            && (bytes[i].is_ascii_digit()
                || bytes[i] == b'.'
                || bytes[i] == b'E'
                || bytes[i] == b'e'
                || bytes[i] == b'-'
                || bytes[i] == b'+')
        {
            if (bytes[i] == b'-' || bytes[i] == b'+')
                && !(bytes[i - 1] == b'E' || bytes[i - 1] == b'e')
            {
                break;
            }
            i += 1;
        }
        if let Ok(value) = input[start..i].parse::<f64>() {
            out.push(value);
        }
    }
    out
}
