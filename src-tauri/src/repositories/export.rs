use libsql_rusqlite::{params, Connection, DatabaseName};
use std::io::{BufReader, Read};

pub const JSON_READ_BUFFER_BYTES: usize = 64 * 1024;
pub const MAX_BODY_CHUNK_CHARACTERS: usize = 49_152;

#[derive(Debug, Clone)]
pub struct CurrentFinalChapter {
    pub payload_rowid: i64,
    pub chapter_id: String,
    pub chapter_number: u64,
    pub title: String,
    pub artifact_id: String,
    pub version: u64,
    pub payload_hash: String,
    pub character_count: u64,
}

const FINAL_CHAPTER_COUNT_SQL: &str = r#"
SELECT COUNT(*)
FROM chapters c
JOIN artifacts a ON a.project_id=c.project_id
  AND a.artifact_id=c.final_artifact_id AND a.version=c.final_version
  AND a.artifact_type='FinalChapter' AND a.artifact_status='COMMITTED'
  AND a.scope_level='CHAPTER' AND json_extract(a.scope_key,'$.chapterId')=c.chapter_id
JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version
  AND p.payload_hash=a.payload_hash
JOIN canonical_heads h ON h.project_id=a.project_id AND h.artifact_type='FinalChapter'
  AND h.artifact_id=a.artifact_id AND h.version=a.version AND h.payload_hash=a.payload_hash
  AND h.scope_level=a.scope_level AND h.scope_key=a.scope_key
WHERE c.project_id=?1 AND c.status='COMMITTED'
"#;

const FINAL_CHAPTERS_SQL: &str = r#"
SELECT p.rowid,c.chapter_id,c.chapter_no,c.title,a.artifact_id,a.version,a.payload_hash
FROM chapters c
JOIN artifacts a ON a.project_id=c.project_id
  AND a.artifact_id=c.final_artifact_id AND a.version=c.final_version
  AND a.artifact_type='FinalChapter' AND a.artifact_status='COMMITTED'
  AND a.scope_level='CHAPTER' AND json_extract(a.scope_key,'$.chapterId')=c.chapter_id
JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version
  AND p.payload_hash=a.payload_hash
JOIN canonical_heads h ON h.project_id=a.project_id AND h.artifact_type='FinalChapter'
  AND h.artifact_id=a.artifact_id AND h.version=a.version AND h.payload_hash=a.payload_hash
  AND h.scope_level=a.scope_level AND h.scope_key=a.scope_key
WHERE c.project_id=?1 AND c.status='COMMITTED'
ORDER BY c.chapter_no,c.chapter_id
"#;

pub fn current_final_chapter_count(
    connection: &Connection,
    project_id: &str,
) -> Result<u64, String> {
    let count: i64 = connection
        .query_row(FINAL_CHAPTER_COUNT_SQL, [project_id], |row| row.get(0))
        .map_err(|_| "EXPORT_CAPABILITY_QUERY_FAILED".to_string())?;
    Ok(count.max(0) as u64)
}

fn current_project_revision(connection: &Connection, project_id: &str) -> Result<u64, String> {
    let revision: i64 = connection
        .query_row(
            "SELECT revision FROM project_meta WHERE project_id=?1 AND maintenance_state='READY'",
            [project_id],
            |row| row.get(0),
        )
        .map_err(|_| "EXPORT_PROJECT_NOT_READY".to_string())?;
    Ok(revision.max(0) as u64)
}

pub fn with_current_final_chapters<T>(
    connection: &Connection,
    project_id: &str,
    use_snapshot: impl FnOnce(u64, Vec<CurrentFinalChapter>) -> Result<T, String>,
) -> Result<T, String> {
    connection
        .execute_batch("BEGIN DEFERRED")
        .map_err(|_| "EXPORT_SOURCE_SNAPSHOT_FAILED".to_string())?;
    let result = (|| {
        let revision = current_project_revision(connection, project_id)?;
        let chapters = current_final_chapters(connection, project_id)?;
        use_snapshot(revision, chapters)
    })();
    match result {
        Ok(value) => {
            if connection.execute_batch("COMMIT").is_ok() {
                Ok(value)
            } else {
                let _ = connection.execute_batch("ROLLBACK");
                Err("EXPORT_SOURCE_SNAPSHOT_FAILED".into())
            }
        }
        Err(error) => {
            let _ = connection.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

fn current_final_chapters(
    connection: &Connection,
    project_id: &str,
) -> Result<Vec<CurrentFinalChapter>, String> {
    let mut statement = connection
        .prepare(FINAL_CHAPTERS_SQL)
        .map_err(|_| "EXPORT_SOURCE_QUERY_FAILED".to_string())?;
    let rows = statement
        .query_map([project_id], |row| {
            Ok(CurrentFinalChapter {
                payload_rowid: row.get(0)?,
                chapter_id: row.get(1)?,
                chapter_number: row.get::<_, i64>(2)?.max(0) as u64,
                title: row.get(3)?,
                artifact_id: row.get(4)?,
                version: row.get::<_, i64>(5)?.max(0) as u64,
                payload_hash: row.get(6)?,
                character_count: 0,
            })
        })
        .map_err(|_| "EXPORT_SOURCE_QUERY_FAILED".to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|_| "EXPORT_SOURCE_QUERY_FAILED".to_string())
}

pub fn stream_final_chapter_body(
    connection: &Connection,
    project_id: &str,
    chapter: &CurrentFinalChapter,
    chunk_characters: usize,
    mut write_chunk: impl FnMut(&str) -> Result<(), String>,
) -> Result<u64, String> {
    if chunk_characters == 0 || chunk_characters > MAX_BODY_CHUNK_CHARACTERS {
        return Err("EXPORT_CHUNK_SIZE_INVALID".into());
    }
    let is_current: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM chapters c JOIN artifacts a ON a.project_id=c.project_id AND a.artifact_id=c.final_artifact_id AND a.version=c.final_version AND a.artifact_type='FinalChapter' AND a.artifact_status='COMMITTED' JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version AND p.payload_hash=a.payload_hash JOIN canonical_heads h ON h.project_id=a.project_id AND h.artifact_type='FinalChapter' AND h.artifact_id=a.artifact_id AND h.version=a.version AND h.payload_hash=a.payload_hash AND h.scope_level=a.scope_level AND h.scope_key=a.scope_key WHERE c.project_id=?1 AND c.chapter_id=?2 AND c.status='COMMITTED' AND c.final_artifact_id=?3 AND c.final_version=?4 AND a.payload_hash=?5 AND p.rowid=?6)",
            params![project_id, chapter.chapter_id, chapter.artifact_id, chapter.version as i64, chapter.payload_hash, chapter.payload_rowid],
            |row| row.get(0),
        )
        .map_err(|_| "EXPORT_SOURCE_READ_FAILED".to_string())?;
    if !is_current {
        return Err("EXPORT_SOURCE_CHANGED".into());
    }
    let blob = connection
        .blob_open(
            DatabaseName::Main,
            "artifact_payloads",
            "payload_json",
            chapter.payload_rowid,
            true,
        )
        .map_err(|_| "EXPORT_SOURCE_READ_FAILED".to_string())?;
    let reader = BufReader::with_capacity(JSON_READ_BUFFER_BYTES, blob);
    let mut cursor = JsonCursor::new(reader);
    stream_top_level_body(&mut cursor, chunk_characters, &mut write_chunk)
}

struct JsonCursor<R> {
    reader: BufReader<R>,
    peeked: Option<u8>,
}

impl<R: Read> JsonCursor<R> {
    fn new(reader: BufReader<R>) -> Self {
        Self {
            reader,
            peeked: None,
        }
    }

    fn next(&mut self) -> Result<Option<u8>, String> {
        if let Some(byte) = self.peeked.take() {
            return Ok(Some(byte));
        }
        let mut byte = [0_u8; 1];
        self.reader
            .read(&mut byte)
            .map_err(|_| "EXPORT_SOURCE_READ_FAILED".to_string())
            .map(|read| (read > 0).then_some(byte[0]))
    }

    fn peek(&mut self) -> Result<Option<u8>, String> {
        if self.peeked.is_none() {
            self.peeked = self.next_from_reader()?;
        }
        Ok(self.peeked)
    }

    fn next_from_reader(&mut self) -> Result<Option<u8>, String> {
        let mut byte = [0_u8; 1];
        self.reader
            .read(&mut byte)
            .map_err(|_| "EXPORT_SOURCE_READ_FAILED".to_string())
            .map(|read| (read > 0).then_some(byte[0]))
    }

    fn expect(&mut self, expected: u8) -> Result<(), String> {
        if self.next()? == Some(expected) {
            Ok(())
        } else {
            Err("EXPORT_PAYLOAD_INVALID".into())
        }
    }

    fn skip_whitespace(&mut self) -> Result<(), String> {
        while matches!(self.peek()?, Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.next()?;
        }
        Ok(())
    }
}

fn stream_top_level_body<R: Read>(
    cursor: &mut JsonCursor<R>,
    chunk_characters: usize,
    write_chunk: &mut impl FnMut(&str) -> Result<(), String>,
) -> Result<u64, String> {
    cursor.skip_whitespace()?;
    cursor.expect(b'{')?;
    loop {
        cursor.skip_whitespace()?;
        if cursor.peek()? == Some(b'}') {
            return Err("EXPORT_BODY_MISSING".into());
        }
        let key = read_json_key(cursor)?;
        cursor.skip_whitespace()?;
        cursor.expect(b':')?;
        cursor.skip_whitespace()?;
        if key == "body" {
            if cursor.peek()? != Some(b'"') {
                return Err("EXPORT_BODY_NOT_TEXT".into());
            }
            return stream_json_string(cursor, chunk_characters, write_chunk);
        }
        skip_json_value(cursor, 0)?;
        cursor.skip_whitespace()?;
        match cursor.next()? {
            Some(b',') => {}
            Some(b'}') => return Err("EXPORT_BODY_MISSING".into()),
            _ => return Err("EXPORT_PAYLOAD_INVALID".into()),
        }
    }
}

fn read_json_key<R: Read>(cursor: &mut JsonCursor<R>) -> Result<String, String> {
    let mut key = String::with_capacity(16);
    let mut too_long = false;
    parse_json_string(cursor, |character| {
        if character == '\0' {
            return Err("EXPORT_PAYLOAD_INVALID".into());
        }
        if key.len() >= 256 {
            too_long = true;
        } else if !too_long {
            key.push(character);
        }
        Ok(())
    })?;
    if too_long {
        Ok(String::new())
    } else {
        Ok(key)
    }
}

fn skip_json_value<R: Read>(cursor: &mut JsonCursor<R>, depth: usize) -> Result<(), String> {
    if depth > 128 {
        return Err("EXPORT_PAYLOAD_INVALID".into());
    }
    cursor.skip_whitespace()?;
    match cursor.peek()? {
        Some(b'"') => parse_json_string(cursor, |_| Ok(())),
        Some(b'{') => {
            cursor.next()?;
            cursor.skip_whitespace()?;
            if cursor.peek()? == Some(b'}') {
                cursor.next()?;
                return Ok(());
            }
            loop {
                read_json_key(cursor)?;
                cursor.skip_whitespace()?;
                cursor.expect(b':')?;
                skip_json_value(cursor, depth + 1)?;
                cursor.skip_whitespace()?;
                match cursor.next()? {
                    Some(b',') => {}
                    Some(b'}') => return Ok(()),
                    _ => return Err("EXPORT_PAYLOAD_INVALID".into()),
                }
            }
        }
        Some(b'[') => {
            cursor.next()?;
            cursor.skip_whitespace()?;
            if cursor.peek()? == Some(b']') {
                cursor.next()?;
                return Ok(());
            }
            loop {
                skip_json_value(cursor, depth + 1)?;
                cursor.skip_whitespace()?;
                match cursor.next()? {
                    Some(b',') => {}
                    Some(b']') => return Ok(()),
                    _ => return Err("EXPORT_PAYLOAD_INVALID".into()),
                }
            }
        }
        Some(b't') => consume_literal(cursor, b"true"),
        Some(b'f') => consume_literal(cursor, b"false"),
        Some(b'n') => consume_literal(cursor, b"null"),
        Some(b'-' | b'0'..=b'9') => skip_number(cursor),
        _ => Err("EXPORT_PAYLOAD_INVALID".into()),
    }
}

fn consume_literal<R: Read>(cursor: &mut JsonCursor<R>, literal: &[u8]) -> Result<(), String> {
    for expected in literal {
        cursor.expect(*expected)?;
    }
    Ok(())
}

fn skip_number<R: Read>(cursor: &mut JsonCursor<R>) -> Result<(), String> {
    let mut consumed = false;
    while matches!(
        cursor.peek()?,
        Some(b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')
    ) {
        cursor.next()?;
        consumed = true;
    }
    if consumed {
        Ok(())
    } else {
        Err("EXPORT_PAYLOAD_INVALID".into())
    }
}

fn parse_json_string<R: Read>(
    cursor: &mut JsonCursor<R>,
    mut receive: impl FnMut(char) -> Result<(), String>,
) -> Result<(), String> {
    cursor.expect(b'"')?;
    loop {
        let first = cursor
            .next()?
            .ok_or_else(|| "EXPORT_PAYLOAD_INVALID".to_string())?;
        let character = match first {
            b'"' => return Ok(()),
            b'\\' => parse_escape(cursor)?,
            0 => return Err("EXPORT_SOURCE_NUL_UNSUPPORTED".into()),
            1..=0x1f => return Err("EXPORT_PAYLOAD_INVALID".into()),
            _ => parse_utf8_character(cursor, first)?,
        };
        receive(character)?;
    }
}

fn stream_json_string<R: Read>(
    cursor: &mut JsonCursor<R>,
    chunk_characters: usize,
    write_chunk: &mut impl FnMut(&str) -> Result<(), String>,
) -> Result<u64, String> {
    let mut chunk = String::with_capacity(chunk_characters.saturating_mul(4));
    let mut chunk_count = 0_usize;
    let mut total = 0_u64;
    parse_json_string(cursor, |character| {
        if character == '\0' {
            return Err("EXPORT_SOURCE_NUL_UNSUPPORTED".into());
        }
        chunk.push(character);
        chunk_count += 1;
        total += 1;
        if chunk_count == chunk_characters {
            write_chunk(&chunk)?;
            chunk.clear();
            chunk_count = 0;
        }
        Ok(())
    })?;
    if !chunk.is_empty() {
        write_chunk(&chunk)?;
    }
    Ok(total)
}

fn parse_escape<R: Read>(cursor: &mut JsonCursor<R>) -> Result<char, String> {
    match cursor
        .next()?
        .ok_or_else(|| "EXPORT_PAYLOAD_INVALID".to_string())?
    {
        b'"' => Ok('"'),
        b'\\' => Ok('\\'),
        b'/' => Ok('/'),
        b'b' => Ok('\u{0008}'),
        b'f' => Ok('\u{000c}'),
        b'n' => Ok('\n'),
        b'r' => Ok('\r'),
        b't' => Ok('\t'),
        b'u' => {
            let high = read_hex_quad(cursor)?;
            let scalar = if (0xd800..=0xdbff).contains(&high) {
                cursor.expect(b'\\')?;
                cursor.expect(b'u')?;
                let low = read_hex_quad(cursor)?;
                if !(0xdc00..=0xdfff).contains(&low) {
                    return Err("EXPORT_PAYLOAD_INVALID".into());
                }
                0x10000 + (((high as u32 - 0xd800) << 10) | (low as u32 - 0xdc00))
            } else if (0xdc00..=0xdfff).contains(&high) {
                return Err("EXPORT_PAYLOAD_INVALID".into());
            } else {
                high as u32
            };
            char::from_u32(scalar).ok_or_else(|| "EXPORT_PAYLOAD_INVALID".into())
        }
        _ => Err("EXPORT_PAYLOAD_INVALID".into()),
    }
}

fn read_hex_quad<R: Read>(cursor: &mut JsonCursor<R>) -> Result<u16, String> {
    let mut value = 0_u16;
    for _ in 0..4 {
        let digit = cursor
            .next()?
            .ok_or_else(|| "EXPORT_PAYLOAD_INVALID".to_string())?;
        value = value
            .checked_mul(16)
            .and_then(|current| current.checked_add((digit as char).to_digit(16)? as u16))
            .ok_or_else(|| "EXPORT_PAYLOAD_INVALID".to_string())?;
    }
    Ok(value)
}

fn parse_utf8_character<R: Read>(cursor: &mut JsonCursor<R>, first: u8) -> Result<char, String> {
    if first < 0x80 {
        return Ok(first as char);
    }
    let width = match first {
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return Err("EXPORT_PAYLOAD_INVALID".into()),
    };
    let mut bytes = [0_u8; 4];
    bytes[0] = first;
    for slot in bytes.iter_mut().take(width).skip(1) {
        let byte = cursor
            .next()?
            .ok_or_else(|| "EXPORT_PAYLOAD_INVALID".to_string())?;
        if !(0x80..=0xbf).contains(&byte) {
            return Err("EXPORT_PAYLOAD_INVALID".into());
        }
        *slot = byte;
    }
    std::str::from_utf8(&bytes[..width])
        .map_err(|_| "EXPORT_PAYLOAD_INVALID".to_string())?
        .chars()
        .next()
        .ok_or_else(|| "EXPORT_PAYLOAD_INVALID".into())
}
