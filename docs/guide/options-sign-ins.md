# Options › Sign-ins

"A user and password used for every download from a host." See
[Sign-ins](sign-ins.md) for every way dm signs in, and [Options](options.md)
for how the window works.

- **Add a sign-in** adds a line with **Host**, **User** and **Password**.
  **Delete** takes it away.
- **Host:** the site's name, such as `files.example.com`. An address pasted
  whole is read for its host. Begin it with a dot (`.example.com`) to cover
  every subdomain too.
- "Sent as a Basic login, or as a Digest where the server asks for one."
- The login is used only for `https://` addresses (and `http://` ones on this
  computer itself), so the password never goes out unencrypted. An address
  with a user and password of its own keeps them.
- The [Grabber](grabber.md) uses a login kept here for its site, and says so.
- "Kept in dm's settings file on this computer, readable by your account."
  On the phone: "Kept in dm's settings file on this phone, where only dm can
  read it."
  Where the system's password store works, the passwords go there and the
  settings file keeps none. **Save settings to a file…** (Options › General)
  never writes them.

This page has no **Restore defaults**: your logins are not a default to go
back to.
