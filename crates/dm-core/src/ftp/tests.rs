// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

#[test]
fn folder_listings_are_read_in_both_forms() {
    let mlsd = "type=cdir;modify=20240101000000; .\r\ntype=pdir; ..\r\ntype=dir;modify=1; pub stuff\r\ntype=file;size=12; a file.zip\r\n";
    assert_eq!(
        parse_mlsd(mlsd),
        vec![
            Listed {
                name: "pub stuff".into(),
                folder: true
            },
            Listed {
                name: "a file.zip".into(),
                folder: false
            },
        ]
    );
    let unix = "drwxr-xr-x   2 ftp ftp   4096 Jan 01 00:00 releases\r\n-rw-r--r--   1 ftp ftp 123456 Jan 01  2024 big file.iso\r\nlrwxrwxrwx 1 ftp ftp 4 Jan 01 00:00 latest -> releases\r\ndrwxr-xr-x 2 ftp ftp 4096 Jan 01 00:00 ..\r\n";
    assert_eq!(
        parse_list(unix),
        vec![
            Listed {
                name: "releases".into(),
                folder: true
            },
            Listed {
                name: "big file.iso".into(),
                folder: false
            },
            Listed {
                name: "latest".into(),
                folder: false
            },
        ]
    );
    let dos = "01-01-24  12:00AM       <DIR>          old\r\n01-01-24  12:00AM               1024 notes.txt\r\n";
    assert_eq!(
        parse_list(dos),
        vec![
            Listed {
                name: "old".into(),
                folder: true
            },
            Listed {
                name: "notes.txt".into(),
                folder: false
            },
        ]
    );
}

#[test]
fn an_ftp_address_is_read_into_its_parts() {
    let password = crate::test_secrets::marked("password");
    let t = Target::parse(&format!(
        "ftp://me:{}@ftp.example.com:2121/pub/a%20file.iso",
        crate::test_secrets::escaped(&password)
    ))
    .unwrap();
    assert_eq!(t.host, "ftp.example.com");
    assert_eq!(t.port, 2121);
    assert_eq!(t.user, "me");
    assert_eq!(t.password, password);
    assert_eq!(t.path, "pub/a file.iso", "relative to the login directory");
    let rooted = Target::parse("ftp://e.com/%2Fetc/motd.txt").unwrap();
    assert_eq!(rooted.path, "/etc/motd.txt", "the root, asked for by %2F");
    let anonymous = Target::parse("ftp://ftp.gnu.org/gnu/hello/hello-2.12.tar.gz").unwrap();
    assert_eq!((anonymous.user.as_str(), anonymous.port), ("anonymous", 21));
    assert!(Target::parse("ftp://ftp.gnu.org/gnu/").is_err(), "a folder");
    assert!(
        Target::parse("ftp://e.com/a%0D%0ADELE%20b").is_err(),
        "a second command hidden in the path"
    );
    assert!(is_ftp("FTP://e.com/f") && is_ftp("ftps://e.com/f") && !is_ftp("http://e.com"));
}

#[test]
fn replies_are_read_as_ftp_writes_them() {
    assert_eq!(reply_code("230 Logged in"), Some((230, true)));
    assert_eq!(reply_code("230-Welcome"), Some((230, false)));
    assert_eq!(reply_code("226"), Some((226, true)));
    assert_eq!(reply_code(" 230 x"), None);
    assert_eq!(reply_code("999 x"), None);
    assert_eq!(
        epsv_port("229 Entering Extended Passive Mode (|||6446|)"),
        Some(6446)
    );
    assert_eq!(
        pasv_port("227 Entering Passive Mode (192,168,1,2,19,137)."),
        Some(19 * 256 + 137)
    );
    assert_eq!(pasv_port("227 Entering Passive Mode (1,2,3,4,999,1)"), None);
    assert_eq!(number_in("213 1048576"), Some(1_048_576));
    assert_eq!(number_in("213"), None);
}

proptest::proptest! {
    #[test]
    fn no_reply_crashes_the_reader(line in "\\PC{0,200}") {
        let _ = reply_code(&line);
        let _ = epsv_port(&line);
        let _ = pasv_port(&line);
        let _ = number_in(&line);
        let _ = Target::parse(&format!("ftp://{line}"));
    }
}
