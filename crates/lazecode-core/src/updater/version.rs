//! Semantic-version comparison for update checks.

use super::*;

/// Compare version strings (`major[.minor[.patch]][-pre][+build]`); missing
/// components are zero and a release sorts above its pre-releases.
pub fn compare_versions(left: &str, right: &str) -> Result<Ordering, UpdateError> {
    Ok(Version::parse(left)?.cmp_parsed(&Version::parse(right)?))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    pre: Vec<PreId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PreId {
    Numeric(u64),
    Alpha(String),
}

impl Version {
    pub(super) fn parse(text: &str) -> Result<Self, UpdateError> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        let text = text.split('+').next().unwrap_or(text);
        let (core, pre) = text.split_once('-').unwrap_or((text, ""));
        let mut parts = core.split('.');
        let major = parse_number(parts.next().unwrap_or(""), text)?;
        let minor = parts
            .next()
            .map(|part| parse_number(part, text))
            .transpose()?
            .unwrap_or(0);
        let patch = parts
            .next()
            .map(|part| parse_number(part, text))
            .transpose()?
            .unwrap_or(0);
        if parts.next().is_some() {
            return Err(UpdateError::Manifest(format!("invalid version {text:?}")));
        }
        let pre = if pre.is_empty() {
            Vec::new()
        } else {
            pre.split('.')
                .map(|identifier| {
                    if !identifier.is_empty() && identifier.bytes().all(|b| b.is_ascii_digit()) {
                        identifier
                            .parse::<u64>()
                            .map(PreId::Numeric)
                            .map_err(|_| UpdateError::Manifest(format!("invalid version {text:?}")))
                    } else {
                        Ok(PreId::Alpha(identifier.to_string()))
                    }
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(Self {
            major,
            minor,
            patch,
            pre,
        })
    }

    fn cmp_parsed(&self, other: &Self) -> Ordering {
        self.major
            .cmp(&other.major)
            .then_with(|| self.minor.cmp(&other.minor))
            .then_with(|| self.patch.cmp(&other.patch))
            .then_with(|| compare_pre(&self.pre, &other.pre))
    }
}

fn parse_number(part: &str, whole: &str) -> Result<u64, UpdateError> {
    part.parse::<u64>()
        .map_err(|_| UpdateError::Manifest(format!("invalid version {whole:?}")))
}

fn compare_pre(left: &[PreId], right: &[PreId]) -> Ordering {
    match (left.is_empty(), right.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        (false, false) => {}
    }
    for (a, b) in left.iter().zip(right) {
        let ordering = match (a, b) {
            (PreId::Numeric(a), PreId::Numeric(b)) => a.cmp(b),
            (PreId::Numeric(_), PreId::Alpha(_)) => Ordering::Less,
            (PreId::Alpha(_), PreId::Numeric(_)) => Ordering::Greater,
            (PreId::Alpha(a), PreId::Alpha(b)) => a.cmp(b),
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left.len().cmp(&right.len())
}
