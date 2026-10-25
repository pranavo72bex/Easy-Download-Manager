//! Describing a file and listing a folder.

use super::*;

/// What the server says about the file.
#[derive(Debug)]
pub(super) struct Described {
    pub(super) size: Option<u64>,
    pub(super) resumable: bool,
    pub(super) validator: Option<String>,
    /// The path from the root, where the file was found there and not
    /// relative to the login folder.
    pub(super) absolute: Option<String>,
}

pub(super) async fn describe(target: &Target) -> Result<Described, Error> {
    let mut session = Session::open(target).await?;
    let (mut code, mut text) = session.command(&format!("SIZE {}", target.path)).await?;
    /* Not there relative to the login folder: asked for from the root, as
     * dm read every FTP address before it read them as RFC 1738 says. A
     * server that does not keep a user in their home folder had that file
     * at `/home/user/f` for a link `ftp://user@host/home/user/f`, and such
     * a link, queued then, still works. */
    let mut absolute = None;
    if code == 550 && !target.path.starts_with('/') {
        let rooted = format!("/{}", target.path);
        let (again, said) = session.command(&format!("SIZE {rooted}")).await?;
        if again == 213 {
            (code, text) = (again, said);
            absolute = Some(rooted);
        }
    }
    let path = absolute.as_deref().unwrap_or(&target.path);
    let size = if code == 213 { number_in(&text) } else { None };
    if code == 550 {
        return Err(refusal(code, &text, &target.path));
    }
    let (code, text) = session.command(&format!("MDTM {path}")).await?;
    let validator = (code == 213)
        .then(|| text.get(4..).unwrap_or("").trim().to_string())
        .filter(|v| !v.is_empty());
    // A server that takes REST starts a transfer where it is told to.
    let (code, _) = session.command("REST 0").await?;
    let _ = session.command("QUIT").await;
    Ok(Described {
        size,
        resumable: code == 350 && size.is_some(),
        validator,
        absolute,
    })
}

/// One thing in an FTP folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub name: String,
    pub folder: bool,
}

/// The most of a folder's listing read: a server sending one without end
/// does not fill memory.
pub(super) const MAX_LISTING: usize = 4 * 1024 * 1024;

/// An `MLSD` listing: `facts; name` a line, the type a fact.
pub(super) fn parse_mlsd(text: &str) -> Vec<Listed> {
    text.lines()
        .filter_map(|line| {
            let (facts, name) = line.split_once(' ')?;
            let kind = facts.split(';').find_map(|f| {
                let (k, v) = f.split_once('=')?;
                k.eq_ignore_ascii_case("type")
                    .then(|| v.to_ascii_lowercase())
            })?;
            let folder = match kind.as_str() {
                "dir" => true,
                "file" => false,
                // The folder itself and its parent.
                _ => return None,
            };
            Some(Listed {
                name: name.to_string(),
                folder,
            })
        })
        .collect()
}

/// A `LIST` listing as Unix servers write it (`drwxr-xr-x 2 u g 4096 Jan 1
/// 00:00 name`) or Windows ones (`01-01-24 12:00AM <DIR> name`).
pub(super) fn parse_list(text: &str) -> Vec<Listed> {
    text.lines()
        .filter_map(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            let (folder, name) =
                if words.first()?.len() == 10 && words.first()?.starts_with(['d', '-', 'l']) {
                    // Unix: the name is everything after the eighth field.
                    let mut rest = line;
                    for _ in 0..8 {
                        rest = rest.trim_start();
                        rest = rest.split_once(char::is_whitespace)?.1;
                    }
                    let name = rest.trim_start();
                    // A link's "name -> target".
                    let name = name.split(" -> ").next().unwrap_or(name);
                    (words.first()?.starts_with('d'), name)
                } else if words.len() >= 4 {
                    let mut rest = line;
                    for _ in 0..3 {
                        rest = rest.trim_start();
                        rest = rest.split_once(char::is_whitespace)?.1;
                    }
                    (*words.get(2)? == "<DIR>", rest.trim_start())
                } else {
                    return None;
                };
            (!name.is_empty() && name != "." && name != "..").then(|| Listed {
                name: name.to_string(),
                folder,
            })
        })
        .collect()
}

/// What an FTP folder holds: its files and folders, by `MLSD` where the
/// server has it, else by `LIST`. Through the proxy the settings name for
/// it, as a download is; never around one.
pub async fn list(url: &str, options: &Options) -> Result<Vec<Listed>, Error> {
    if let Some(why) = options.no_route_through_system_proxy(url, "FTP") {
        return Err(Error::Internal(why));
    }
    let mut target = Target::parse_folder(url)?;
    target.route = Route::parse(options.proxy_for(url))?;
    let mut session = Session::open(&target).await?;
    let folder = target.path.trim_end_matches('/').to_string();
    for (verb, parse) in [
        ("MLSD", parse_mlsd as fn(&str) -> Vec<Listed>),
        ("LIST", parse_list),
    ] {
        let data = session.data(&folder).await?;
        let command = if folder.is_empty() {
            verb.to_string()
        } else {
            format!("{verb} {folder}")
        };
        let (code, text) = session.command(&command).await?;
        if code != 150 && code != 125 {
            drop(data);
            // Not a command this server knows: the older one.
            if verb == "MLSD" && (500..=504).contains(&code) {
                continue;
            }
            return Err(refusal(code, &text, &folder));
        }
        let data = session.secure(data).await?;
        let mut bytes = Vec::new();
        timed(async {
            let mut data = data;
            (&mut data)
                .take(MAX_LISTING as u64)
                .read_to_end(&mut bytes)
                .await
        })
        .await?;
        let _ = session.reply().await;
        let _ = session.command("QUIT").await;
        return Ok(parse(&String::from_utf8_lossy(&bytes)));
    }
    Err(Error::Internal(
        "the FTP server would not list the folder".into(),
    ))
}
