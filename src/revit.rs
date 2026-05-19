use std::{
    env, fmt, fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RevitVersion {
    V2025,
    V2026,
    V2027,
}

impl RevitVersion {
    pub fn year(self) -> u16 {
        match self {
            Self::V2025 => 2025,
            Self::V2026 => 2026,
            Self::V2027 => 2027,
        }
    }

    pub fn ifc_tag(self) -> &'static str {
        match self {
            Self::V2025 => "IFC_v25.4.40",
            Self::V2026 => "IFC_v26.4.1",
            Self::V2027 => "IFC_v27.0.1.1",
        }
    }

    pub fn parse_year(value: &str) -> Option<Self> {
        match value {
            "2025" => Some(Self::V2025),
            "2026" => Some(Self::V2026),
            "2027" => Some(Self::V2027),
            _ => None,
        }
    }
}

impl fmt::Display for RevitVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.year())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevitInstallation {
    pub version: RevitVersion,
    pub install_dir: PathBuf,
    pub revit_exe: PathBuf,
}

pub fn detect_revit_installations() -> Vec<RevitInstallation> {
    let mut roots = Vec::new();
    if let Ok(program_files) = env::var("ProgramFiles") {
        roots.push(PathBuf::from(program_files).join("Autodesk"));
    }
    if let Ok(program_w6432) = env::var("ProgramW6432") {
        roots.push(PathBuf::from(program_w6432).join("Autodesk"));
    }
    roots.push(PathBuf::from(r"C:\Program Files\Autodesk"));
    detect_revit_installations_in_roots(roots.iter().map(PathBuf::as_path))
}

pub fn revit_installation_from_exe(
    revit_exe: &Path,
    requested_version: Option<RevitVersion>,
) -> Option<RevitInstallation> {
    if !revit_exe
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("Revit.exe"))
    {
        return None;
    }
    let version = requested_version.or_else(|| parse_version_from_path(revit_exe))?;
    let install_dir = revit_exe.parent()?.to_path_buf();
    Some(RevitInstallation {
        version,
        install_dir,
        revit_exe: revit_exe.to_path_buf(),
    })
}

pub fn missing_revit_installation_message() -> String {
    [
        "找不到 Revit 2025/2026/2027 安裝；RVT 轉 IFC 需要合法本機 Revit。",
        "",
        "請安裝 Autodesk Revit 後再重試：",
        "  已有授權/訂閱：Autodesk Account https://manage.autodesk.com/products",
        "  試用版：Autodesk Revit Free Trial https://www.autodesk.com/products/revit/free-trial",
        "",
        r"預期預設路徑類似：C:\Program Files\Autodesk\Revit 2026\Revit.exe",
        r#"若 Revit 裝在非預設路徑，請加：--revit-exe "D:\...\Revit.exe" --revit-version 2026"#,
    ]
    .join("\n")
}

pub fn detect_revit_installations_in_roots<'a>(
    roots: impl IntoIterator<Item = &'a Path>,
) -> Vec<RevitInstallation> {
    let mut installs = Vec::new();
    for root in roots {
        for version in [
            RevitVersion::V2027,
            RevitVersion::V2026,
            RevitVersion::V2025,
        ] {
            let install_dir = root.join(format!("Revit {}", version.year()));
            let revit_exe = install_dir.join("Revit.exe");
            if revit_exe.is_file() {
                installs.push(RevitInstallation {
                    version,
                    install_dir,
                    revit_exe,
                });
            }
        }
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                let install_dir = entry.path();
                if !install_dir.is_dir() {
                    continue;
                }
                let Some(version) = parse_version_from_install_dir(&install_dir) else {
                    continue;
                };
                let revit_exe = install_dir.join("Revit.exe");
                if revit_exe.is_file() {
                    installs.push(RevitInstallation {
                        version,
                        install_dir,
                        revit_exe,
                    });
                }
            }
        }
    }
    installs.sort_by(|a, b| {
        std::cmp::Reverse(a.version)
            .cmp(&std::cmp::Reverse(b.version))
            .then_with(|| a.revit_exe.cmp(&b.revit_exe))
    });
    installs.dedup_by(|a, b| a.version == b.version && a.revit_exe == b.revit_exe);
    installs
}

pub fn best_revit_installation() -> Option<RevitInstallation> {
    detect_revit_installations().into_iter().next()
}

fn parse_version_from_path(path: &Path) -> Option<RevitVersion> {
    for component in path.ancestors() {
        let Some(text) = component.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        for version in [
            RevitVersion::V2027,
            RevitVersion::V2026,
            RevitVersion::V2025,
        ] {
            if text.contains(&version.year().to_string()) {
                return Some(version);
            }
        }
    }
    None
}

fn parse_version_from_install_dir(path: &Path) -> Option<RevitVersion> {
    let text = path.file_name()?.to_str()?.to_ascii_lowercase();
    if !text.contains("revit") {
        return None;
    }
    for version in [
        RevitVersion::V2027,
        RevitVersion::V2026,
        RevitVersion::V2025,
    ] {
        if text.contains(&version.year().to_string()) {
            return Some(version);
        }
    }
    None
}
