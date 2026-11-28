# A second network for faster downloads

EDM can download over two networks at once: the one your computer normally uses, and a
second one you choose in **Options › Advanced › Network › Second network**. Half of each
download's connections then go over the second network, so a download can be as fast as both
lines together. Once a second is chosen, **Third network** beside it adds one more line, and
each download's connections are then shared among all three.

## When it helps, and when it does not

It helps only when the second network is **a different internet line**:

- **Helps:** your home broadband (Wi-Fi or a cable), and your phone's mobile data over USB.
  These are two separate lines, so their speeds add up.
- **Helps:** a wired office line and a separate mobile hotspot.
- **Does not help:** Wi-Fi and a network cable to **the same router**. Both end in the same
  internet line, so it is no faster.
- **Does not help:** your phone's hotspot over Wi-Fi while the computer's Wi-Fi is also your
  home connection. A computer's Wi-Fi joins one network at a time, so the phone replaces your
  home Wi-Fi rather than adding to it. Use the phone over its USB cable instead.

Mobile data may be limited or paid by the gigabyte. Watch your plan, and use **Options ›
Advanced › Network** to turn the second network off when you are done.

## Connecting a phone over USB

Keep your computer on its normal connection (Wi-Fi or cable). The phone becomes the second
network.

### iPhone

1. On the iPhone, open **Settings › Personal Hotspot** and turn on **Allow Others to Join**.
2. Connect it to the computer with its cable. If it asks whether to trust this computer, tap
   **Trust**.
3. **macOS:** the iPhone shows as **iPhone USB** in System Settings › Network. If it is not
   there, add it with the **…** menu there.
   **Windows:** Windows needs Apple's own driver for this, which comes with the Apple Devices
   app (or iTunes) from the Microsoft Store.
   **Linux:** most systems take it as a wired connection by themselves.
4. In EDM, choose it in **Options › Advanced › Network › Second network**.

### Android

1. Connect the phone to the computer with its cable.
2. On the phone, open **Settings › Network & internet › Hotspot & tethering** (the words differ
   a little between phones) and turn on **USB tethering**.
3. **Windows and Linux:** the phone shows as a new wired connection.
   **macOS:** macOS does not take most Android phones' USB tethering by itself. Use an iPhone,
   or a second line of another kind.
4. In EDM, choose it in **Options › Advanced › Network › Second network**.

## If it does not seem faster

- Check that the second network has its own internet line (above).
- The second network is not used while a proxy is on, or while **Only through** is chosen.
- A server may limit how fast one person can download, whatever the lines.
