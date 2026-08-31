# e7

Epic Seven Secret Shop auto-refresher over adb (Linux).

## Install

    just build          # cargo build --release + copy to ~/.local/bin/e7

Needs `adb` on PATH and a paired device (`adb connect ip:5555` or USB).
Works alongside a running scrcpy session.

## Use

Open the Secret Shop in game, then:

    e7 run --refreshes 100            # 300 skystones, buys covenant + mystic
    e7 run --skystones 300 --buy cov,mys,fb
    e7 run --dry-run --refreshes 5 -v # detect only, no taps
    e7 devices
    e7 screenshot                     # 1920x1080 png for cropping templates

Phones that are not 16:9 get a temporary `wm size 1080x1920` override
(game shows black bars); it is reset when e7 exits. Ctrl+C stops after
the current step; twice aborts.

History: `~/.local/share/e7/history.csv`, one row per purchase.

## Templates

`assets/{cov,mys,fb}.png` are 141x113 crops of the item icon box from a
1920x1080 screenshot. When the game changes item art, crop a new one from
`e7 screenshot` output and pass `--templates-dir` (or replace the asset and
rebuild).
