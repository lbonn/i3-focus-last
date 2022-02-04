i3-focus-last
=============

[![Crate](https://meritbadge.herokuapp.com/i3-focus-last)](https://crates.io/crates/i3-focus-last)

Another implementation of this classic (and useful) example of i3 ipc use.

Works on reasonable versions of i3 and sway >= 1.2.

Done in [rust](https://www.rust-lang.org):

* for fun!
* it's compiled, so we won't suffer from the overhead of starting a new
  interpreter for each client (the python version I used was sluggish at
  times, for this reason)

Usage
-----

Add this line to your i3 or Sway configuration:

```
exec_always i3-focus-last server
```

Then, add a binding to execute `i3-focus-last`:

```
bindsym $mod+Tab exec i3-focus-last <--ignore-scratchpad> <--hide-scratchpad>
```

Options
--------
- `--ignore-scratchpad` - Don't focus to/from scratchpad
- `--hide-scratchpad` - If scratchpad is focused, hide it and focus previous window

Menu mode
---------

i3-focus-last can be used with [rofi](https://github.com/davatorium/rofi) to display a window switcher menu in which the entries are sorted by focus order.

To launch it, just run `i3-focus-last menu` when the server is running (or bind it to some key combination).

It can also shows icons corresponding to the window class or app_id. This mapping can be customized by modifying `~/.config/i3-focus-last/icons.json`. For example:

```
{
  "Alacritty": "terminal",
  "firefox": "firefox",
  "Chromium": "chromium"
}
```

The values in the dictionary should be names of icons in `/usr/share/icons/**`.
