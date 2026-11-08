//! Readers for what the projects publish: the FreeBSD support table and `MANIFEST`, the NetBSD
//! release pages and `SHA512` files, and the version files inside each tree.
//!
//! The pages are generated, so a reader looks for the few marks it needs and fails loudly when
//! they move.

/// One row of the FreeBSD security page's table of supported branches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Supported {
    /// `stable/15` or `releng/15.1`.
    pub branch: String,
    /// `15.1` for a release row, `None` for a stable branch.
    pub release: Option<String>,
    /// The release date, `YYYY-MM-DD`.
    pub released: Option<String>,
    /// The expected end of life, `YYYY-MM-DD`.
    pub eol: Option<String>,
}

/// The rows of the table under `id="sup"` on <https://www.freebsd.org/security/>.
pub fn freebsd_supported(html: &str) -> Result<Vec<Supported>, String> {
    let start = html
        .find("id=\"sup\"")
        .ok_or("the FreeBSD security page has no id=\"sup\" table")?;
    let rest = &html[start..];
    let body =
        between(rest, "<tbody>", "</tbody>").ok_or("the FreeBSD support table has no <tbody>")?;
    let mut rows = Vec::new();
    for row in body.split("<tr>").skip(1) {
        let cells: Vec<String> = row
            .split("<td")
            .skip(1)
            .map(|cell| text(cell.split("</td>").next().unwrap_or_default()))
            .collect();
        if cells.len() < 4 {
            return Err(format!("a FreeBSD support row has {} cells", cells.len()));
        }
        let release = cells[1]
            .strip_suffix("-RELEASE")
            .map(std::string::ToString::to_string);
        rows.push(Supported {
            branch: cells[0].clone(),
            release,
            released: english_date(&cells[2]),
            eol: english_date(&cells[3]),
        });
    }
    if rows.is_empty() {
        return Err("the FreeBSD support table is empty".into());
    }
    Ok(rows)
}

/// The sha256 of one distribution file in a FreeBSD `MANIFEST`.
#[must_use]
pub fn freebsd_manifest(text: &str, file: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let mut fields = line.split('\t');
        (fields.next() == Some(file))
            .then(|| fields.next())
            .flatten()
            .filter(|sha| is_hex(sha, 64))
            .map(std::string::ToString::to_string)
    })
}

/// The supported releases on <https://www.netbsd.org/releases/>, as `(major, version)`.
pub fn netbsd_supported(html: &str) -> Result<Vec<(String, String)>, String> {
    let start = html
        .find("id=\"supported\"")
        .ok_or("the NetBSD releases page has no id=\"supported\" section")?;
    let rest = &html[start..];
    let end = rest[1..]
        .find("class=\"sect1\"")
        .map_or(rest.len(), |e| e + 1);
    let mut found = Vec::new();
    for piece in rest[..end].split("href=\"formal-").skip(1) {
        let link = piece.split('"').next().unwrap_or_default();
        let Some((major, page)) = link.split_once('/') else {
            continue;
        };
        let Some(version) = page
            .strip_prefix("NetBSD-")
            .and_then(|p| p.strip_suffix(".html"))
        else {
            continue;
        };
        let pair = (major.to_string(), version.to_string());
        if !found.contains(&pair) {
            found.push(pair);
        }
    }
    if found.is_empty() {
        return Err("the NetBSD releases page lists no supported release".into());
    }
    Ok(found)
}

/// The date in the title of a NetBSD release announcement, such as
/// `Announcing NetBSD 11.1 (November 2, 2026)`.
#[must_use]
pub fn netbsd_announced(html: &str) -> Option<String> {
    let title = between(html, "<title>", "</title>")?;
    let inside = between(title, "(", ")")?;
    english_date(inside)
}

/// The lines of a BSD-style `SHA512` file, as `(file, digest)`.
#[must_use]
pub fn bsd_sums(text: &str, algorithm: &str) -> Vec<(String, String)> {
    let prefix = format!("{algorithm} (");
    text.lines()
        .filter_map(|line| {
            let rest = line.strip_prefix(&prefix)?;
            let (file, sum) = rest.split_once(") = ")?;
            Some((file.to_string(), sum.trim().to_string()))
        })
        .collect()
}

/// `15.1-RELEASE` or `16.0-CURRENT` from `sys/conf/newvers.sh`.
pub fn freebsd_newvers(text: &str) -> Result<String, String> {
    let field = |name: &str| {
        text.lines().find_map(|l| {
            l.strip_prefix(name)
                .and_then(|v| v.strip_prefix('='))
                .map(|v| v.trim().trim_matches('"').to_string())
        })
    };
    match (field("REVISION"), field("BRANCH")) {
        (Some(revision), Some(branch)) => Ok(format!("{revision}-{branch}")),
        _ => Err("newvers.sh has no REVISION or BRANCH".into()),
    }
}

/// `11.99.10` from the comment after `__NetBSD_Version__` in `sys/sys/param.h`.
pub fn netbsd_param(text: &str) -> Result<String, String> {
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("#define") else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix("__NetBSD_Version__") else {
            continue;
        };
        if let Some(comment) = between(rest, "/* NetBSD ", "*/") {
            return Ok(comment.trim().to_string());
        }
    }
    Err("param.h has no __NetBSD_Version__ line".into())
}

/// The first line of GCC's `BASE-VER`.
pub fn gcc_base_ver(text: &str) -> Result<String, String> {
    let version = text.lines().next().unwrap_or_default().trim();
    if version.split('.').count() == 3 && version.split('.').all(|p| p.parse::<u32>().is_ok()) {
        Ok(version.to_string())
    } else {
        Err(format!("BASE-VER is {version:?}"))
    }
}

/// `June 16, 2026` as `2026-06-16`. Anything else, such as `n/a`, is `None`.
#[must_use]
pub fn english_date(text: &str) -> Option<String> {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let mut words = text.split_whitespace();
    let month = words.next()?;
    let month = MONTHS.iter().position(|m| *m == month)? + 1;
    let day: u32 = words.next()?.trim_end_matches(',').parse().ok()?;
    let year: u32 = words.next()?.parse().ok()?;
    Some(format!("{year:04}-{month:02}-{day:02}"))
}

/// Whether `text` is `len` lowercase hex digits.
#[must_use]
pub fn is_hex(text: &str, len: usize) -> bool {
    text.len() == len
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn between<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = text.find(open)? + open.len();
    let len = text[start..].find(close)?;
    Some(&text[start..start + len])
}

/// The text of a cell that starts inside its `<td` tag, with tags dropped and space collapsed.
fn text(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = true;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write as _;

    fn row(cells: [&str; 4]) -> String {
        let mut s = String::from("<tr>\n");
        for cell in cells {
            let _ = writeln!(
                s,
                "<td class=\"tableblock halign-left valign-top\"><p class=\"tableblock\">{cell}</p></td>"
            );
        }
        s + "</tr>\n"
    }

    #[test]
    fn the_freebsd_support_table() {
        let html = format!(
            "<table><tbody><tr><td>decoy</td></tr></tbody></table>\
             <h2 id=\"sup\">Supported FreeBSD releases</h2><table><thead><tr><th>Branch</th></tr></thead><tbody>\n{}{}</tbody></table>",
            row(["stable/15", "n/a", "n/a", "December 31, 2029"]),
            row([
                "releng/15.1",
                "15.1-RELEASE",
                "June 16, 2026",
                "March 31, 2027"
            ]),
        );
        let rows = freebsd_supported(&html).unwrap();
        assert_eq!(
            rows,
            [
                Supported {
                    branch: "stable/15".into(),
                    release: None,
                    released: None,
                    eol: Some("2029-12-31".into()),
                },
                Supported {
                    branch: "releng/15.1".into(),
                    release: Some("15.1".into()),
                    released: Some("2026-06-16".into()),
                    eol: Some("2027-03-31".into()),
                },
            ]
        );
        assert!(freebsd_supported("<html></html>").is_err());
    }

    #[test]
    fn a_manifest_line() {
        let sha = "cf5762da53fd52e1eaf0f9ceee9bf58cbe314c821031d0d9ffa76823185a89e1";
        let text = format!(
            "base.txz\t{}\t30469\tbase\t\"Base system\"\ton\nsrc.txz\t{sha}\t99\tsrc\t\"Source\"\toff\n",
            "0".repeat(64)
        );
        assert_eq!(freebsd_manifest(&text, "src.txz").as_deref(), Some(sha));
        assert_eq!(freebsd_manifest(&text, "ports.txz"), None);
    }

    #[test]
    fn the_netbsd_releases_page() {
        let html = r#"<a href="formal-11/">x</a>
<div class="sect1" id="supported"><h2>Supported Releases</h2>
The <a class="ulink" href="formal-11/" target="_top">NetBSD 11.x</a> series:
<li><a class="ulink" href="formal-11/NetBSD-11.1.html" target="_top">NetBSD 11.1</a></li>
<li><a class="ulink" href="formal-10/NetBSD-10.2.html" target="_top">NetBSD 10.2</a></li>
</div><div class="sect1" id="eol"><a href="formal-9/NetBSD-9.4.html">9.4</a></div>"#;
        assert_eq!(
            netbsd_supported(html).unwrap(),
            [
                ("11".to_string(), "11.1".to_string()),
                ("10".to_string(), "10.2".to_string())
            ]
        );
        assert_eq!(
            netbsd_announced("<title>Announcing NetBSD 11.1 (November 2, 2026)</title>").as_deref(),
            Some("2026-11-02")
        );
    }

    #[test]
    fn sums_and_versions() {
        let sums = bsd_sums(
            "SHA512 (src.tgz) = abc\nSHA512 (xsrc.tgz) = def\nMD5 (src.tgz) = 1\n",
            "SHA512",
        );
        assert_eq!(
            sums,
            [
                ("src.tgz".into(), "abc".into()),
                ("xsrc.tgz".into(), "def".into())
            ]
        );
        assert_eq!(
            freebsd_newvers("TYPE=\"FreeBSD\"\nREVISION=\"15.2\"\nBRANCH=\"BETA1\"\n").unwrap(),
            "15.2-BETA1"
        );
        assert_eq!(
            netbsd_param(
                " *\t#define __NetBSD_Version__ MMmmrrpp00\n#define\t__NetBSD_Version__\t1199001000\t/* NetBSD 11.99.10 */\n"
            )
            .unwrap(),
            "11.99.10"
        );
        assert_eq!(gcc_base_ver("14.4.0\n").unwrap(), "14.4.0");
        assert!(gcc_base_ver("<html>").is_err());
        assert_eq!(english_date("n/a"), None);
    }
}
