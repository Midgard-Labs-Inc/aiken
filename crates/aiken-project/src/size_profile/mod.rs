//! Optional size diagnostics for an already-built Aiken project.
mod flat;
#[cfg(test)]
mod tests;

use crate::{Project, blueprint::validator::Validator, telemetry::EventListener};
use aiken_lang::ast::Tracing;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::Path,
};

// Keep compiledCode as written: decoding it into a Program can normalize its bytes.
#[derive(serde::Deserialize)]
struct ScriptCode {
    #[serde(rename = "compiledCode")]
    compiled_code: String,
}

#[derive(serde::Deserialize)]
struct BlueprintCode {
    validators: Vec<Validator<ScriptCode>>,
}

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn ensure(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message).into())
    }
}

fn find_source(functions: &[Value], name: &str) -> Option<Value> {
    // Generated span_<start>_<end> identifiers do not identify a unique module.
    // Only an unambiguous, surviving function name is attributed to source.
    let matches: Vec<_> = functions
        .iter()
        .filter(|f| {
            let prefix = f["uplc_name"].as_str().unwrap_or("");
            name == prefix
                || name
                    .strip_prefix(prefix)
                    .is_some_and(|s| s.starts_with('_'))
        })
        .collect();
    let longest = matches
        .iter()
        .map(|f| f["uplc_name"].as_str().unwrap_or("").len())
        .max()?;
    let matches: Vec<_> = matches
        .into_iter()
        .filter(|f| f["uplc_name"].as_str().unwrap_or("").len() == longest)
        .collect();
    if matches.len() == 1 {
        Some(matches[0]["source"].clone())
    } else {
        None
    }
}

fn build_report(measured: &flat::Measurement, functions: &[Value]) -> Value {
    let mut children = vec![Vec::new(); measured.nodes.len()];
    let mut kinds = BTreeMap::<&str, usize>::new();
    for node in &measured.nodes {
        if let Some(parent) = node.parent {
            children[parent].push(node.id);
        }
        *kinds.entry(node.kind).or_default() += node.exclusive_bits;
    }
    let mut sections = Vec::new();
    for node in &measured.nodes {
        if node.kind != "Apply" || children[node.id].len() != 2 {
            continue;
        }
        let binding = &measured.nodes[children[node.id][0]];
        if binding.kind != "Lambda" {
            continue;
        }
        let Some(name) = binding.name.as_deref() else {
            continue;
        };
        let definition = &measured.nodes[children[node.id][1]];
        sections.push(json!({
            "name": name, "source": find_source(functions, name),
            "definition_term_id": definition.id, "definition_bits": definition.inclusive_bits,
            "binding_overhead_bits": node.exclusive_bits + binding.exclusive_bits,
        }));
    }
    sections.sort_by_key(|r| std::cmp::Reverse(r["definition_bits"].as_u64().unwrap_or(0)));
    json!({
        "flat_bytes": measured.flat.len(), "cbor_bytes": measured.cbor.len(),
        "blueprint_bytes_match": true, "version_bits": measured.version_bits,
        "final_padding_bits": measured.padding_bits,
        "term_bits": measured.nodes[0].inclusive_bits,
        "sections": sections, "exclusive_bits_by_uplc_node_kind": kinds,
        "nodes": measured.nodes,
    })
}

/// Profile the checked modules using the same tracing options as the blueprint.
pub(crate) fn write_report<T: EventListener>(
    project: &Project<T>,
    blueprint_path: &Path,
    output: &Path,
    tracing: Tracing,
) -> Result<usize> {
    let blueprint_path = blueprint_path.canonicalize()?;
    if let Ok(report_path) = output.canonicalize() {
        ensure(
            report_path != blueprint_path,
            "Size report would overwrite the blueprint",
        )?;
    }
    let blueprint: BlueprintCode = serde_json::from_slice(&fs::read(&blueprint_path)?)?;
    let modules = &project.checked_modules;
    let functions: Vec<_> = modules.functions().filter_map(|(module, function)| {
        let (_, lines) = project.module_sources.get(&module.name)?;
        Some(json!({
            "uplc_name": format!("{}_{}", module.name, function.name),
            "source": {
                "path": module.input_path.strip_prefix(&project.root).unwrap_or(&module.input_path).to_string_lossy(),
                "start_line": lines.line_number(function.location.start),
                "end_line": lines.line_number(function.end_position),
            },
        }))
    }).collect();
    let mut generator = project.new_generator(tracing);
    let mut seen = BTreeSet::new();
    let mut reports = Vec::new();
    for (module, validator) in modules.validators() {
        let handlers: Vec<_> = blueprint
            .validators
            .iter()
            .filter(|handler| {
                handler.get_module_and_name() == (module.name.as_str(), validator.name.as_str())
            })
            .collect();
        if handlers.is_empty() {
            continue;
        }
        let program = generator.generate(validator, &module.name)?;
        let measured = flat::measure_program(program)?;
        let mut data = build_report(&measured, &functions);
        for handler in &handlers {
            ensure(seen.insert(&handler.title), "Duplicate validator title")?;
            ensure(
                hex::decode(&handler.program.compiled_code)? == measured.cbor,
                &format!("Blueprint bytes differ for {}", handler.title),
            )?;
        }
        data["title"] = json!(format!("{}.{}", module.name, validator.name));
        data["handlers"] = json!(
            handlers
                .iter()
                .map(|handler| &handler.title)
                .collect::<Vec<_>>()
        );
        reports.push(data);
    }
    ensure(
        seen.len() == blueprint.validators.len(),
        "Not all blueprint validators were profiled",
    )?;
    let report = json!({
        "schema_version": 1,
        "compiler_version": aiken_lang::version::compiler_version(true),
        "validators": reports,
    });
    let contents = serde_json::to_vec_pretty(&report)?;
    // Only replace the report after every handler has passed its byte comparison.
    fs::create_dir_all(output.parent().ok_or("Missing report directory")?)?;
    fs::write(output, contents)?;
    Ok(seen.len())
}
