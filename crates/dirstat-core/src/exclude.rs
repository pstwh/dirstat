use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct Excludes {
    pub names: Vec<Vec<u8>>,
    pub paths: Vec<PathBuf>,
}

impl Excludes {
    pub fn parse(patterns: &[String]) -> Excludes {
        let mut excludes = Excludes::default();
        for pattern in patterns.iter().map(|p| p.trim()).filter(|p| !p.is_empty()) {
            let path = Path::new(pattern);
            if path.is_absolute() {
                excludes.paths.push(path.to_path_buf());
            } else {
                excludes.names.push(pattern.as_bytes().to_vec());
            }
        }
        excludes
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty() && self.paths.is_empty()
    }

    pub fn matches(&self, parent: &Path, name: &[u8]) -> bool {
        if self.names.iter().any(|pattern| glob(pattern, name)) {
            return true;
        }
        if self.paths.is_empty() {
            return false;
        }
        let name = unsafe { std::ffi::OsStr::from_encoded_bytes_unchecked(name) };
        let full = parent.join(name);
        self.paths.contains(&full)
    }
}

pub fn glob(pattern: &[u8], text: &[u8]) -> bool {
    let (mut p, mut t) = (0usize, 0usize);
    let mut star: Option<usize> = None;
    let mut mark = 0usize;
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p].eq_ignore_ascii_case(&text[t])) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            mark = t;
        } else if let Some(position) = star {
            p = position + 1;
            mark += 1;
            t = mark;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_match_names() {
        assert!(glob(b"node_modules", b"node_modules"));
        assert!(glob(b"*.log", b"server.LOG"));
        assert!(glob(b"cache?", b"cache1"));
        assert!(glob(b"*", b""));
        assert!(!glob(b"*.log", b"server.txt"));
        assert!(!glob(b"target", b"targets"));
    }

    #[test]
    fn absolute_patterns_match_full_paths() {
        let excludes = Excludes::parse(&["/home/x/.cache".into(), ".git".into()]);
        assert!(excludes.matches(Path::new("/home/x"), b".cache"));
        assert!(excludes.matches(Path::new("/anywhere"), b".git"));
        assert!(!excludes.matches(Path::new("/home/y"), b".cache"));
    }
}
