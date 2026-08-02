use kitmux_model::{
    CommandId, SplitNode, SshProfile, SshResolution, decode_control_request, decode_settings,
    decode_snapshot, decode_ssh_profiles, encode_control_request, encode_settings, encode_snapshot,
    encode_ssh_profiles,
};
use serde_json::{Value, json};
use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = env::args_os().skip(1);
    let fixtures = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("usage: export_portable_fixtures FIXTURE_DIR OUTPUT_JSON")?;
    let output = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("usage: export_portable_fixtures FIXTURE_DIR OUTPUT_JSON")?;
    if arguments.next().is_some() {
        return Err("usage: export_portable_fixtures FIXTURE_DIR OUTPUT_JSON".into());
    }

    let state = fixture(&fixtures, "state-snapshots.json")?;
    let state_cases = cases(&state)
        .filter(|case| matches!(case["disposition"].as_str(), Some("accept" | "repair")))
        .map(|case| {
            let snapshot = decode_snapshot(&serde_json::to_vec(&case["input"])?)?;
            Ok(json!({
                "id": case["id"],
                "value": serde_json::from_slice::<Value>(&encode_snapshot(snapshot)?)?
            }))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;

    let settings = fixture(&fixtures, "settings.json")?;
    let settings_cases = cases(&settings)
        .map(|case| {
            let document = decode_settings(&serde_json::to_vec(&case["input"])?)?;
            Ok(json!({
                "id": case["id"],
                "value": serde_json::from_slice::<Value>(&encode_settings(&document)?)?
            }))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;

    let splits = fixture(&fixtures, "split-tree.json")?;
    let split_cases = cases(&splits)
        .filter(|case| case["disposition"] == "accept")
        .map(|case| {
            let tree: SplitNode = serde_json::from_value(case["initial"].clone())?;
            let leaf_order = tree
                .pane_ids()
                .into_iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>();
            Ok(json!({"id": case["id"], "value": tree, "leafOrder": leaf_order}))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;

    let control = fixture(&fixtures, "control-protocol.json")?;
    let control_cases = cases(&control)
        .filter(|case| case["disposition"] == "accept")
        .map(|case| {
            let request = decode_control_request(&serde_json::to_vec(&case["input"])?)?;
            Ok(json!({
                "id": case["id"],
                "value": serde_json::from_slice::<Value>(&encode_control_request(&request)?)?
            }))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;

    let ssh = fixture(&fixtures, "ssh-profile-review.json")?;
    let mut ssh_documents = Vec::new();
    let mut ssh_reviews = Vec::new();
    for case in cases(&ssh) {
        if case["disposition"] != "accept" {
            continue;
        }
        if !case["input"].is_null() {
            let document = decode_ssh_profiles(&serde_json::to_vec(&case["input"])?)?;
            ssh_documents.push(json!({
                "id": case["id"],
                "value": serde_json::from_slice::<Value>(&encode_ssh_profiles(document)?)?
            }));
        } else {
            let profile: SshProfile = serde_json::from_value(case["profile"].clone())?;
            let resolution = SshResolution::parse(
                &profile.host_alias,
                case["resolutionOutput"]
                    .as_str()
                    .ok_or("SSH resolution output is not text")?,
            )
            .ok_or("SSH resolution fixture is invalid")?;
            ssh_reviews.push(json!({
                "id": case["id"],
                "profile": profile,
                "resolutionOutput": case["resolutionOutput"],
                "review": resolution.review(&profile)
            }));
        }
    }

    let bundle = json!({
        "version": 1,
        "stateCases": state_cases,
        "settingsCases": settings_cases,
        "splitCases": split_cases,
        "commands": CommandId::ALL.iter().map(|command| command.as_str()).collect::<Vec<_>>(),
        "controlCases": control_cases,
        "sshDocuments": ssh_documents,
        "sshReviews": ssh_reviews
    });
    fs::write(output, serde_json::to_vec_pretty(&bundle)?)?;
    Ok(())
}

fn fixture(root: &Path, name: &str) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_slice(&fs::read(root.join(name))?)?)
}

fn cases(document: &Value) -> impl Iterator<Item = &Value> {
    document["cases"].as_array().into_iter().flatten()
}
