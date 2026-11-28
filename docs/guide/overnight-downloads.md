# Downloads overnight

How to have downloads wait for the night and run by themselves, step by
step.

1. Open the [Scheduler](scheduler.md): the toolbar's **Scheduler** button, or
   **Downloads › Scheduler…**.
2. Under **Queues**, click **Main queue**.
3. Tick **Only download between set hours**.
4. Set **Start at** to the evening hour, for example 23:00, and **Stop at**
   to the morning hour, for example 07:00. Hours that end before they start
   run overnight.
5. Under **Every week, on the days ticked**, tick the nights you want.
6. Under **When the scheduled downloads are done**, choose what EDM does
   after: **Do nothing**, **Exit EDM**, **Sleep**, **Hibernate** or **Turn off
   computer**.
7. Click **OK**.
8. Add your downloads with **Add for later** in
   [Download details](download-details.md), in the **Main queue**.
9. Leave EDM running (it can stay in the tray). At the start hour the queue
   starts by itself; at the stop hour what is still running stops, and goes
   on the next night.

For a single night, tick **Once, on this date** instead of the days, and
pick the date. To start the queue before its hour, click **Start now** in the
Scheduler, or **Start queue** on the toolbar.
