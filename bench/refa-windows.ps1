# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# Answers reference A's windows as a person would during a benchmark run: Yes to its own question
# box titled -Title exactly, No to the others carrying that title (offers and reminders), and
# closes its download-finished, file-information, error and save boxes. Touches only the windows
# of the process named -Process. Prints what it pressed or closed. Run by bench.mjs, never with a
# blocking call (the harness's server shares its event loop).
param([Parameter(Mandatory = $true)][string]$Process, [Parameter(Mandatory = $true)][string]$Title)
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public class BenchW {
  public delegate bool Cb(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(Cb cb, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr p, Cb cb, IntPtr l);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
}
"@
$ids = @((Get-Process $Process -ErrorAction SilentlyContinue).Id)
$closed = @()
$press = { param($h, $pattern, $what)
  [BenchW]::EnumChildWindows($h, { param($k, $m)
    $q = New-Object System.Text.StringBuilder 64; [BenchW]::GetWindowText($k, $q, 64) | Out-Null
    if ($q.ToString() -match $pattern) { [BenchW]::SendMessage($k, 0xF5, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null; $script:closed += $what }
    $true }, [IntPtr]::Zero) | Out-Null }
[BenchW]::EnumWindows({ param($h, $l)
  $p = 0; [BenchW]::GetWindowThreadProcessId($h, [ref]$p) | Out-Null
  if ($ids -contains $p -and [BenchW]::IsWindowVisible($h)) {
    $s = New-Object System.Text.StringBuilder 256; [BenchW]::GetWindowText($h, $s, 256) | Out-Null
    $t = $s.ToString()
    $c = New-Object System.Text.StringBuilder 256; [BenchW]::GetClassName($h, $c, 256) | Out-Null
    $dialog = $c.ToString() -eq "#32770"
    if ($dialog -and $t -eq $Title) { & $press $h '^&?Yes$' 'question-Yes' }
    if ($dialog -and $t.Contains($Title)) { & $press $h '^&?No$' 'offer-No' }
    if ($t -match 'Download complete|Download File Info|^dl$|^page.zip$|^missing.bin$|lie.bin|File Info|Download error|Error|Already exists|Save As|Authentication') {
      [BenchW]::PostMessage($h, 0x10, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null; $script:closed += $t
    }
  }
  $true }, [IntPtr]::Zero) | Out-Null
$closed -join '; '
