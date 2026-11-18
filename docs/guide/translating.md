# Translating dm

dm's window speaks English and thirteen other languages. Each language is one
file of the English and its translation, and anyone who speaks a language can
mend it, read what no native speaker has read yet, or start a new one. You need
no Rust to do it, though the check below runs best with it.

## Where the words are

| File | What it holds |
| --- | --- |
| `desktop/assets/i18n/<code>.json` | A language's catalog: each English string the window says, and its translation |
| `desktop/assets/i18n/unread/<code>.json` | The English of each string in that catalog no native speaker has read yet |
| `desktop/assets/i18n/template/catalog.json` | Every English string, each with an empty translation: the file a new language starts from |
| `extension/_locales/<code>/messages.json` | The browser extension's words, in the browser's own format |

A catalog is keyed by the English itself:

```json
{
 "Open": "Öffnen",
 "{} of {}": "{} von {}",
 "{} file||{} files": "{} Datei||{} Dateien"
}
```

Change only the right-hand side. The English is what the code asks for; a
changed English string is a string the window no longer finds, and it shows in
English.

## Check a catalog

From the `desktop/` folder (with Rust from <https://rustup.rs>):

```sh
cargo xtask i18n            # every language: translated, missing, unread
cargo xtask i18n de         # one language: each missing and each unread string
cargo xtask i18n --issue de # the issue a German reader can take (see below)
```

A string is **missing** when the catalog lacks it or leaves its translation
empty; the window shows it in English. A string is **unread** while it is in
the language's unread list. Neither fails the command: they are work still to
do. The command fails, and names the line, only when a file is malformed: it is
not JSON of the right shape, it gives a string twice, a translation takes other
values than its English, or an unread list names a string the catalog does not
have.

`cargo test -p xtask` runs the same check on every catalog, so a pull request's
own checks catch a malformed file too.

## The rules a translation keeps

- **Values.** `{}` is filled with a value, the first `{}` with the first, and
  so on. If your language wants them in another order, number them: `{1}` is
  the second value and `{0}` the first, so `"{} of {}"` may become
  `"{1} 中的 {0}"`. A translation takes the same values as its English, each
  once or more; the check fails on one that loses or adds a value.
- **Counts.** `||` divides the forms a sentence takes for a number:
  `"{} file||{} files"`. Write as many forms as your language's grammar has,
  in the order the [CLDR plural rules](https://www.unicode.org/cldr/charts/latest/supplemental/language_plural_rules.html)
  give them (one, few, many, other): Russian and Polish three, German two,
  Japanese and Turkish one. Use no `||` where the English has none.
- **Line breaks** are written `\n`, and a `"` inside a translation is `\"`.
- **dm** is dm in every language. Keep the names of keys (Ctrl, Shift), of
  menus a system draws, and of file formats as the system shows them in your
  language.
- **Short and plain.** The English is short on purpose: buttons and menu items
  have little room. A translation as plain as the English reads better than a
  word-for-word one.

## Read a language: the unread strings

Some translations were written without a native speaker; they are listed in
`desktop/assets/i18n/unread/<code>.json`. The maintainer posts one issue per
language with those strings (made by `cargo xtask i18n --issue <code>`). To
take one:

1. Say in the issue that you are reading it, so two people do not do the same
   work. Reading part of the list helps too.
2. For each string, read the translation in `<code>.json` beside its English.
   Mend it if it reads wrong, too long or unnatural.
3. Mark it read: delete its line from `unread/<code>.json`, mended or not.
   Keep the file a JSON list: no comma after the last string. When every line
   is gone the file may be deleted; a language with no unread file has nothing
   unread.
4. Run `cargo xtask i18n <code>` and send a pull request that names the issue.

Any other string you find wrong, unread or not, can be mended the same way.

## Start a new language

1. Name it by its language code, as [BCP 47](https://www.rfc-editor.org/info/bcp47)
   writes it: the two-letter language (`hi` for Hindi, `vi` for Vietnamese,
   `uk` for Ukrainian), and a region only where the language differs by it
   (`pt-BR`, `zh-CN`).
2. Copy `desktop/assets/i18n/template/catalog.json` to
   `desktop/assets/i18n/<code>.json`. Every translation in it is empty, so
   `cargo xtask i18n <code>` starts with every string missing.
3. Fill in the translations. A file part done is welcome: the check shows what
   is left, and someone else can finish it.
4. Copy `extension/_locales/en/messages.json` to
   `extension/_locales/<code>/messages.json`, writing the code's `-` as `_`
   (`pt_BR`), and translate each `"message"`. Leave each `"description"`: it
   says where the words are shown. A word the window and the extension both
   say is translated alike in both.
5. Send a pull request. The maintainer then adds the language to the list
   dm offers (View › Language and Options › General), with its plural rule and
   its name in itself, and checks the window draws it right before it ships.

Languages dm lacks that many people speak: Hindi, Vietnamese, Ukrainian and
Arabic.

## Right to left

Arabic, Hebrew, Persian and Urdu wait for gaps row U35: the window must be
mirrored, its lists and panels laid out from the right, before a right-to-left
catalog can ship. A catalog for one is still welcome, and is held until then.
