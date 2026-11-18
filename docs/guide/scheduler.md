# Scheduler

The Scheduler sets the hours when a queue's downloads may run, and what dm
does once the scheduled downloads are done. Open it with **Downloads ›
Scheduler…**, the toolbar's **Scheduler** button, or **Scheduler…** on the
tray icon's menu. It opens with the queue the list shows chosen. Asked for
again while it is open, it comes to the front as it stands.

Outside a queue's hours nothing in it starts, and what is running is
stopped. When the hours come round, the downloads the schedule stopped
start again by themselves; a download you stopped yourself stays stopped.

## Queues

The left side lists the **Queues**. Click one to show its hours on the
right. You can move between queues and change several before you click
**OK**.

- **Start now:** starts the chosen queue at once.
- **Stop:** stops the chosen queue at once.

These two act straight away; they do not wait for **OK**.

## Hours

- For the **Main queue** the window says "The hours when downloads may
  run, in every queue without hours of its own."
- For any other queue, **Hours of its own for this queue:** unticked, the
  window says "This queue follows the Main queue's hours." and the rest is
  greyed. Tick it to give the queue its own hours.
- **Only download between set hours:** the hours are used only while this
  is ticked. Unticked, the queue's downloads may run at any time, and the
  choices under it are greyed.
- **Start at** and **Stop at:** the hour and the minute, each picked from a
  list (with AM and PM where the computer uses a 12-hour clock). Hours that
  end before they start run overnight.
- **Every week, on the days ticked:** tick the days, **Sun** to **Sat**. The
  day is the one the hours began on: Friday's hours from 23:00 to 07:00
  still count as Friday at 2 in the morning.
- **Once, on this date:** one run on the day, month and year you pick,
  starting at **Start at** and ending at **Stop at** (the next morning when
  **Stop at** is not later than **Start at**).
- **Wake the computer from sleep to start:** on Windows only. dm wakes a
  sleeping computer when the hours begin.

## When the downloads are done

- **When the scheduled downloads are done:** **Do nothing**, **Exit dm**,
  **Sleep**, **Hibernate** or **Turn off computer**. A choice this computer
  gives dm no way to do is greyed (macOS cannot hibernate). When the hours
  begin with downloads waiting, dm sets this choice as **Downloads › When
  done**, unless you chose one there yourself since dm started. Once the
  queue has nothing left to run, dm counts down 30 seconds ("Downloads are
  done" in [Questions dm asks](questions.md)) and then does it. If the hours
  end first, the choice is put away again.
- **Try a failed download again this many times (0 for none):** once
  nothing is left to run, dm tries the downloads that failed again, up to
  this many rounds (at most 5) while the hours last.
- **Run this program when they are done (optional):** a program, with any
  arguments, that dm runs once the downloads are done, after the tries
  again.
- **Hang up modem when done:** on Windows only. Hangs up the dial-up or VPN
  connection set in **Options › Advanced** once the downloads are done.

The note at the foot says "HH:MM, local time. A window ending before it
starts runs overnight."

## Buttons

- **Cancel:** closes the Scheduler and keeps none of your changes (what
  **Start now** and **Stop** did stays done).
- **OK:** keeps the hours of every queue you changed and closes the
  Scheduler.

If a queue's hours cannot be kept, the Scheduler shows that queue and says
why, and stays open:

- "… to …: times are HH:MM in 24 hours, such as 01:00 or 23:30."
- "… is not a date: write it as YYYY-MM-DD, such as 2026-12-31."
- "Tick at least one day."

## Good to know

- **Return** is **OK**; **Escape**, Ctrl+W (⌘W on macOS) and the close
  button are **Cancel**.
- While the hours hold downloads back, the status bar says "waiting for
  the schedule (opens at …)". **Start now** or **Resume** on such a
  download asks **Start outside the schedule?**.
- How many downloads run at once is set in **Options › Downloads**, not
  here.
