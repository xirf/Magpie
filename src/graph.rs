//! A bounded syntax graph, not a complete compiler-resolved call graph.
use crate::{config::Config, store::Store};
use anyhow::{Context, Result, ensure};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};
use tree_sitter::{Node, Parser};

fn read(path: &Path) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    ensure!(file.metadata()?.is_file(), "Not a regular file");
    let mut bytes = Vec::new();
    file.take(262_145).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 262_144, "Source exceeds 256 KiB");
    Ok(bytes)
}

fn setup(store: &Store) -> Result<()> {
    store.0.execute_batch("CREATE TABLE IF NOT EXISTS code_files(path TEXT PRIMARY KEY, hash TEXT NOT NULL, errors INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS code_edges(path TEXT NOT NULL, source TEXT NOT NULL, relation TEXT NOT NULL, target TEXT NOT NULL, line INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS code_edges_path ON code_edges(path);")?;
    // Parser/grammar dependency changes invalidate derived edges, even for unchanged source.
    let fingerprint = format!(
        "syntax-v2:{:x}",
        Sha256::digest(include_bytes!("../Cargo.lock"))
    );
    if store.get("syntax_fingerprint")?.as_deref() != Some(&fingerprint) {
        store.0.execute("UPDATE code_files SET hash=''", [])?;
        store.set("syntax_fingerprint", &fingerprint)?;
    }
    Ok(())
}

pub fn index(store: &Store, config: &Config, path: &Path) -> Result<Value> {
    setup(store)?;
    let mut parsed = 0;
    let mut unchanged = 0;
    let mut skipped = Vec::new();
    let mut visited = 0;
    let mut limited = false;
    let entries = walkdir::WalkDir::new(path)
        .follow_links(false)
        .max_depth(32)
        .into_iter()
        .filter_entry(|entry| {
            !matches!(
                entry.file_name().to_str(),
                Some(".git" | "target" | "node_modules" | ".mornye" | "dist")
            ) && entry.path() != config.state_dir
        });
    for entry in entries {
        visited += 1;
        if visited > 5000 {
            limited = true;
            break;
        }
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let ext = entry
            .path()
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or("");
        if !matches!(ext, "rs" | "ts" | "tsx") {
            continue;
        }
        if parsed + unchanged >= 500 {
            limited = true;
            break;
        }
        match index_file(store, entry.path(), ext) {
            Ok(true) => parsed += 1,
            Ok(false) => unchanged += 1,
            Err(error) => {
                if skipped.len() < 20 {
                    skipped.push(json!({"path":entry.path(),"error":error.to_string()}));
                }
            }
        }
    }
    Ok(
        json!({"parsed":parsed,"unchanged":unchanged,"skipped":skipped,"limit_reached":limited,"note":"Syntax edges only. No type resolution, macro expansion, or proof of runtime calls."}),
    )
}

fn index_file(store: &Store, path: &Path, ext: &str) -> Result<bool> {
    let path = std::fs::canonicalize(path)?;
    let key = path.to_string_lossy();
    let bytes = read(&path)?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let existing: Option<String> = store
        .0
        .query_row(
            "SELECT hash FROM code_files WHERE path=?1",
            [key.as_ref()],
            |r| r.get(0),
        )
        .optional()?;
    if existing.as_deref() == Some(&hash) {
        return Ok(false);
    }
    let mut parser = Parser::new();
    let language = match ext {
        "rs" => tree_sitter_rust::LANGUAGE,
        "tsx" => tree_sitter_typescript::LANGUAGE_TSX,
        _ => tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
    };
    parser.set_language(&language.into())?;
    let tree = parser
        .parse(&bytes, None)
        .context("Unable to parse source")?;
    let mut edges = Vec::new();
    walk(tree.root_node(), &bytes, "<file>", 0, &mut edges)?;
    let tx = store.0.unchecked_transaction()?;
    tx.execute("DELETE FROM code_edges WHERE path=?1", [key.as_ref()])?;
    tx.execute("INSERT INTO code_files VALUES(?1,?2,?3) ON CONFLICT(path) DO UPDATE SET hash=excluded.hash,errors=excluded.errors",params![key,hash,tree.root_node().has_error()])?;
    for (source, relation, target, line) in edges {
        tx.execute(
            "INSERT INTO code_edges VALUES(?1,?2,?3,?4,?5)",
            params![key, source, relation, target, line as i64],
        )?;
    }
    tx.commit()?;
    Ok(true)
}

type Edge = (String, &'static str, String, usize);
fn walk(
    node: Node<'_>,
    bytes: &[u8],
    scope: &str,
    depth: usize,
    edges: &mut Vec<Edge>,
) -> Result<()> {
    ensure!(
        depth <= 128 && edges.len() < 4000,
        "Syntax graph exceeds per-file complexity limit"
    );
    let kind = node.kind();
    let definition = matches!(
        kind,
        "function_item"
            | "struct_item"
            | "enum_item"
            | "trait_item"
            | "function_declaration"
            | "method_definition"
            | "class_declaration"
            | "interface_declaration"
            | "type_alias_declaration"
    );
    let name = if definition {
        node.child_by_field_name("name")
            .and_then(|n| n.utf8_text(bytes).ok())
    } else {
        None
    };
    if let Some(name) = name {
        edges.push((
            scope.into(),
            "defines",
            name.into(),
            node.start_position().row + 1,
        ));
    }
    let scope = name.unwrap_or(scope);
    let relation = match kind {
        "call_expression" => node.child_by_field_name("function").map(|n| ("calls", n)),
        "use_declaration" | "import_statement" => Some(("imports", node)),
        _ => None,
    };
    if let Some((relation, target)) = relation {
        let text: String = target.utf8_text(bytes)?.chars().take(300).collect();
        edges.push((scope.into(), relation, text, node.start_position().row + 1));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        walk(child, bytes, scope, depth + 1, edges)?;
    }
    Ok(())
}

pub fn search(store: &Store, query: &str) -> Result<Value> {
    setup(store)?;
    ensure!(!query.trim().is_empty(), "Supply a symbol or import name");
    let mut statement = store.0.prepare("SELECT e.path,e.source,e.relation,e.target,e.line,f.hash,f.errors FROM code_edges e JOIN code_files f ON e.path=f.path WHERE instr(lower(e.source),lower(?1))>0 OR instr(lower(e.target),lower(?1))>0 ORDER BY e.path,e.line LIMIT 30")?;
    let rows = statement.query_map([query], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, bool>(6)?,
        ))
    })?;
    let mut results = Vec::new();
    for row in rows {
        let (path, source, relation, target, line, hash, errors) = row?;
        let stale = read(Path::new(&path))
            .map(|bytes| format!("{:x}", Sha256::digest(bytes)) != hash)
            .unwrap_or(true);
        results.push(json!({"path":path,"source":source,"relation":relation,"target":target,"line":line,"file_hash":hash,"provenance":"EXTRACTED_SYNTAX","stale":stale,"parse_errors":errors}));
    }
    Ok(
        json!({"edges":results,"note":"Stale edges must be reindexed before use. Calls are syntactic references, not resolved runtime destinations."}),
    )
}
