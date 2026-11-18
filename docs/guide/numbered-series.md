# Add numbered series

Add numbered series makes a run of addresses that differ only by a number
or a letter, such as `photo1.jpg` to `photo40.jpg`, and adds them all. Open
it with **Tasks › Add numbered series…** or Ctrl+Shift+N (⌘⇧N on macOS).

The window says: "Put * where the number or letter changes, as in
example.com/photo*.jpg".

## Fields

- **Address with *:** the address, with one `*` where the number or letter
  goes.
- **From** and **To:** the first and last number (such as 1 and 40), or the
  first and last letter (`a` to `z`, or `A` to `Z`).
- **Digits:** how many digits each number has, 0 to 9. Hold the mouse over
  the word for "How many digits each number has: 3 makes 7 into 007; 0
  leaves it as it is".

As you type, the line under the fields says how many addresses the series
makes and the first one ("40 addresses, the first
https://example.com/photo01.jpg"), and the box below lists every one of
them. Nothing is fetched until you click a button. Before you type, the box
says "Each address the series makes is listed here."

When the series cannot be made, the line says why:

- "The address needs exactly one * to fill in."
- "The range runs backwards." **From** is after **To**.
- "Ten thousand addresses is more than a batch." The series makes more than
  10,000 addresses.
- "From and To must both be numbers, or both letters of one case."

## Buttons

- **Add for later:** adds every address to the Main queue without starting
  them.
- **Cancel:** adds nothing.
- **Start now:** adds every address and starts them.

**Add for later** and **Start now** are greyed until **Address with *** has
something in it. Once added, the status bar says "Added … downloads."

## Keys

- **Return** in any of the fields is **Start now**.
- **Escape**, Ctrl+W (⌘W on macOS) and the close button are **Cancel**.
- Asked for again while it is open, the window comes to the front as it
  stands.
