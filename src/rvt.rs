use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

use crate::{
    revit::{
        RevitInstallation, RevitVersion, detect_revit_installations,
        missing_revit_installation_message, revit_installation_from_exe,
    },
    rvt_job::{RvtExportJob, RvtExportOptions},
};

const JOB_ENV_VAR: &str = "RVT_TO_GLB_JOB";
const ADDIN_ID: &str = "7c9a3071-4a4b-4da5-9d5d-4f33a6cf41f4";

#[derive(Debug, Clone)]
pub struct RvtToIfcOptions {
    pub input_rvt: PathBuf,
    pub output_ifc: PathBuf,
    pub requested_version: Option<RevitVersion>,
    pub revit_exe: Option<PathBuf>,
    pub bridge_assembly: Option<PathBuf>,
    pub timeout: Duration,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RvtExportResult {
    success: bool,
    output_ifc: Option<PathBuf>,
    message: Option<String>,
}

pub fn export_rvt_to_ifc(options: &RvtToIfcOptions) -> Result<PathBuf> {
    if !options.input_rvt.is_file() {
        bail!("找不到 RVT 檔案：{}", options.input_rvt.display());
    }
    let bridge_revit_exe = options
        .bridge_assembly
        .as_deref()
        .and_then(revit_exe_from_install_dir_argument);
    let install = select_revit_installation(
        options.requested_version,
        options.revit_exe.as_deref().or(bridge_revit_exe.as_deref()),
    )?;
    let bridge_assembly =
        resolve_bridge_assembly(options.bridge_assembly.as_deref(), install.version)?;

    if let Some(parent) = options.output_ifc.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("建立 IFC 輸出目錄失敗：{}", parent.display()))?;
    }
    let (job_json, result_json) = prepare_sidecars_for_new_export(&options.output_ifc)?;
    let job = RvtExportJob {
        input_rvt: options.input_rvt.clone(),
        output_ifc: options.output_ifc.clone(),
        result_json: result_json.clone(),
        options: RvtExportOptions::default(),
    };
    fs::write(&job_json, serde_json::to_vec_pretty(&job)?)
        .with_context(|| format!("寫入 Revit job 失敗：{}", job_json.display()))?;
    let addin_manifest = install_revit_addin_manifest(&install, &bridge_assembly)?;

    let mut child = Command::new(&install.revit_exe)
        .env(JOB_ENV_VAR, &job_json)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("啟動 Revit 失敗：{}", install.revit_exe.display()))?;

    let started = Instant::now();
    loop {
        if result_json.is_file() {
            let result: RvtExportResult =
                serde_json::from_slice(&fs::read(&result_json).with_context(|| {
                    format!("讀取 Revit result 失敗：{}", result_json.display())
                })?)?;
            if result.success {
                let output = result
                    .output_ifc
                    .unwrap_or_else(|| options.output_ifc.clone());
                if output.is_file() {
                    return Ok(output);
                }
                bail!("Revit 回報成功，但找不到 IFC：{}", output.display());
            }
            bail!(
                "Revit IFC export 失敗：{}",
                result
                    .message
                    .unwrap_or_else(|| "unknown error".to_string())
            );
        }
        if let Some(status) = child.try_wait()? {
            bail!(
                "Revit 在輸出 result 前結束，exit status: {status}，addin manifest: {}",
                addin_manifest.display()
            );
        }
        if started.elapsed() > options.timeout {
            let _ = child.kill();
            bail!("Revit IFC export 超時：{}", options.input_rvt.display());
        }
        thread::sleep(Duration::from_secs(2));
    }
}

fn select_revit_installation(
    requested: Option<RevitVersion>,
    explicit_revit_exe: Option<&Path>,
) -> Result<RevitInstallation> {
    if let Some(revit_exe) = explicit_revit_exe {
        if !revit_exe.is_file() {
            bail!(
                "找不到 --revit-exe 指定的 Revit.exe：{}",
                revit_exe.display()
            );
        }
        return revit_installation_from_exe(revit_exe, requested).ok_or_else(|| {
            anyhow!(
                "無法判斷 --revit-exe 的 Revit 版本：{}。請同時指定 --revit-version 2025/2026/2027。",
                revit_exe.display()
            )
        });
    }

    let installs = detect_revit_installations();
    if let Some(version) = requested {
        return installs
            .into_iter()
            .find(|install| install.version == version)
            .ok_or_else(|| anyhow!("找不到 Revit {version} 安裝"));
    }
    installs
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!(missing_revit_installation_message()))
}

fn default_bridge_assembly(version: RevitVersion) -> PathBuf {
    let exe_dir = env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));
    if let Some(versioned) = exe_dir.parent().and_then(Path::parent).map(|root| {
        root.join("target")
            .join("revit_bridge")
            .join(version.year().to_string())
            .join("RvtToGlb.RevitIfcExporter.dll")
    }) && versioned.is_file()
    {
        return versioned;
    }

    exe_dir
        .join("revit_bridge")
        .join("RvtToGlb.RevitIfcExporter.dll")
}

fn resolve_bridge_assembly(requested: Option<&Path>, version: RevitVersion) -> Result<PathBuf> {
    let default = default_bridge_assembly(version);
    let Some(requested) = requested else {
        if default.is_file() {
            return Ok(default);
        }
        bail!("{}", bridge_assembly_not_found_message(&default, &default));
    };

    if requested.is_file() {
        return Ok(requested.to_path_buf());
    }

    let requested_dll = requested.join("RvtToGlb.RevitIfcExporter.dll");
    if requested_dll.is_file() {
        return Ok(requested_dll);
    }

    if revit_exe_from_install_dir_argument(requested).is_some() {
        if default.is_file() {
            return Ok(default);
        }
        bail!(
            "{}",
            revit_install_dir_bridge_missing_message(requested, &default)
        );
    }

    bail!("{}", bridge_assembly_not_found_message(requested, &default));
}

fn revit_exe_from_install_dir_argument(path: &Path) -> Option<PathBuf> {
    if !path.is_dir() {
        return None;
    }
    let revit_exe = path.join("Revit.exe");
    if revit_exe.is_file() {
        Some(revit_exe)
    } else {
        None
    }
}

fn bridge_assembly_not_found_message(requested: &Path, default: &Path) -> String {
    format!(
        "找不到 Revit bridge DLL：{}。--bridge-assembly 需要指向 RvtToGlb.RevitIfcExporter.dll，或指向含有該 DLL 的資料夾。\n\
若你要指定 Revit 安裝位置，請使用 --revit-exe \"...\\Revit.exe\"；若傳入的是 Revit 安裝資料夾，工具也會嘗試自動使用該資料夾內的 Revit.exe。\n\
請先建置 bridge：pwsh -NoProfile -ExecutionPolicy Bypass -File .\\tools\\build_revit_bridge.ps1 -Version 2027 -Configuration Release\n\
預設 bridge 位置：{}",
        requested.display(),
        default.display()
    )
}

fn revit_install_dir_bridge_missing_message(revit_install_dir: &Path, default: &Path) -> String {
    format!(
        "已偵測到 Revit 安裝資料夾：{}，會自動使用其 Revit.exe；但找不到本工具的 Revit bridge DLL：{}。\n\
請先建置 bridge：pwsh -NoProfile -ExecutionPolicy Bypass -File .\\tools\\build_revit_bridge.ps1 -Version 2027 -Configuration Release",
        revit_install_dir.display(),
        default.display()
    )
}

fn prepare_sidecars_for_new_export(output_ifc: &Path) -> Result<(PathBuf, PathBuf)> {
    let result_json = output_ifc.with_extension("rvt-export-result.json");
    if result_json.exists() {
        fs::remove_file(&result_json)
            .with_context(|| format!("刪除舊 Revit result 失敗：{}", result_json.display()))?;
    }
    Ok((
        output_ifc.with_extension("rvt-export-job.json"),
        result_json,
    ))
}

fn install_revit_addin_manifest(
    install: &RevitInstallation,
    bridge_assembly: &Path,
) -> Result<PathBuf> {
    let appdata = env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("APPDATA 未設定，無法安裝 Revit add-in manifest"))?;
    let addin_dir = appdata
        .join("Autodesk")
        .join("Revit")
        .join("Addins")
        .join(install.version.year().to_string());
    fs::create_dir_all(&addin_dir)
        .with_context(|| format!("建立 Revit add-in 目錄失敗：{}", addin_dir.display()))?;
    let manifest = addin_dir.join("RvtToGlb.RevitIfcExporter.addin");
    let content = format!(
        r#"<?xml version="1.0" encoding="utf-8" standalone="no"?>
<RevitAddIns>
  <AddIn Type="Application">
    <Name>RVT to GLB IFC Exporter</Name>
    <Assembly>{}</Assembly>
    <AddInId>{}</AddInId>
    <FullClassName>RvtToGlb.RevitIfcExporter.ExportApplication</FullClassName>
    <VendorId>RVTG</VendorId>
    <VendorDescription>RVT to IFC bridge for Rust RVT to GLB converter</VendorDescription>
  </AddIn>
</RevitAddIns>
"#,
        bridge_assembly.display(),
        ADDIN_ID
    );
    fs::write(&manifest, content)
        .with_context(|| format!("寫入 Revit add-in manifest 失敗：{}", manifest.display()))?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::{
        bridge_assembly_not_found_message, prepare_sidecars_for_new_export,
        resolve_bridge_assembly, revit_exe_from_install_dir_argument,
    };
    use crate::revit::RevitVersion;

    #[test]
    fn prepare_sidecars_removes_stale_result_before_launching_revit() {
        let temp = tempfile::tempdir().expect("tempdir");
        let output_ifc = temp.path().join("model.ifc");
        let stale_result = output_ifc.with_extension("rvt-export-result.json");
        std::fs::write(&stale_result, r#"{"success":false}"#).expect("stale result");

        let (_job_json, result_json) =
            prepare_sidecars_for_new_export(&output_ifc).expect("prepare sidecars");

        assert_eq!(result_json, stale_result);
        assert!(!result_json.exists());
    }

    #[test]
    fn bridge_assembly_error_explains_revit_install_dir_mixup() {
        let temp = tempfile::tempdir().expect("tempdir");
        let revit_install_dir = temp.path().join("Revit 2027");
        std::fs::create_dir_all(&revit_install_dir).expect("create fake revit dir");
        std::fs::write(revit_install_dir.join("Revit.exe"), b"").expect("fake Revit.exe");

        let default_bridge = temp.path().join("target").join("revit_bridge").join("2027");
        let message = bridge_assembly_not_found_message(&revit_install_dir, &default_bridge);

        assert!(message.contains("--bridge-assembly 需要指向"));
        assert!(message.contains("RvtToGlb.RevitIfcExporter.dll"));
        assert!(message.contains("--revit-exe"));
        assert!(message.contains("Revit.exe"));
    }

    #[test]
    fn revit_install_dir_argument_provides_revit_exe_hint() {
        let temp = tempfile::tempdir().expect("tempdir");
        let revit_install_dir = temp.path().join("Autodesk").join("Revit 2026");
        std::fs::create_dir_all(&revit_install_dir).expect("create fake revit dir");
        let revit_exe = revit_install_dir.join("Revit.exe");
        std::fs::write(&revit_exe, b"").expect("fake Revit.exe");

        assert_eq!(
            revit_exe_from_install_dir_argument(&revit_install_dir),
            Some(revit_exe)
        );
    }

    #[test]
    fn bridge_assembly_can_be_directory_containing_bridge_dll() {
        let temp = tempfile::tempdir().expect("tempdir");
        let bridge_dir = temp.path().join("bridge");
        std::fs::create_dir_all(&bridge_dir).expect("create bridge dir");
        let bridge_dll = bridge_dir.join("RvtToGlb.RevitIfcExporter.dll");
        std::fs::write(&bridge_dll, b"bridge").expect("fake bridge dll");

        let resolved =
            resolve_bridge_assembly(Some(&bridge_dir), RevitVersion::V2027).expect("bridge dir");

        assert_eq!(resolved, bridge_dll);
    }
}
