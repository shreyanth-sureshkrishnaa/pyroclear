# Formatting
The config file has only 2 sections, `color` and `animation`.

## `color`
`color` has 3 valid variables, `pallete`, `from` and `to`.
You need either `pallete` or `from` and `to`

| Name | Type | Example |
| :--- | :--- | :--- |
| `pallete` | string | `"fire"` |
| `from` | string | `"#ff0000"` |
| `to` | string | `"#00ff00"` |

### `pallete`
The selected pallete, you can check the available pallets with:
```
pyroclear --list-colors
```
If you use `pallet`, you can't use `from` nor `to`

### `from`
The starting color, in hex (`#rrggbb`).
If you use `from`, you need to also use `to` and you can't use `pallete`

### `to`
The ending color, in hex (`#rrggbb`).
If you use `to`, you need to also use `from` and you can't use `pallete`

## `animation`
`animation` has 16 valid variables, `fps`, `wind`, `height`, `direction`, `duration`, `flames_duration`, `effect`, `wave_speed`, `wave_angle`, `vortex_outward`, `vortex_speed`, `vortex_clockwise`, `explosion_count`, `explosion_speed`, `explosion_rhythm`, and `explosion_debris`

| Name | Type | Example |
| :--- | :--- | :--- |
| `fps` | integer | `60` |
| `wind` | integer | `0` |
| `height` | integer | `1` |
| `direction` | boolean | `false` |
| `duration` | float | `1.0` |
| `flames_duration` |  float | `0.7` |
| `effect` | string | `"wave"` |
| `wave_speed` | integer | `3` |
| `wave_angle` | integer | `-20` |
| `vortex_outward` | boolean | `false` |
| `vortex_speed` | integer | `3` |
| `vortex_clockwise` | boolean | `true` |
| `explosion_count` | integer | `3` |
| `explosion_speed` | integer | `3` |
| `explosion_rhythm` | integer | `1` |
| `explosion_debris` | integer | `1` |

### `fps`
The frames per second of the animation, also afects speed.
Can be any positive number.
### `wind`
A wind that moves the top of the fire.
Can be any integer from -2 to 2.
### `height`
The height of the fire.
Can be any integer from 0 to 3.
### `direction`
The direction of the fire, `false` means from bottom to top, `true` is from top to bottom.
Can be either `true` or `false`.
### `duration`
The duration of the animation.
Can be any positive decimal.
### `flames_duration`
The duration of the flames.
Can be any positive decimal.
### `effect`
The clear effect: `"fire"` (default), `"wave"`, `"vortex"`, `"explosion"`, or `"random"` (a different one each run).
`fps` and `duration` apply to every effect; `wind`, `height`, `direction` and `flames_duration` only affect `fire`.
### `wave_speed`
How fast the `wave` rolls in and out, from `1` (Very Slow) to `5` (Very Fast). Default `3`.
It scales `duration` for the wave only.
### `wave_angle`
Tilt of the `wave` crest, in degrees, from `-30` to `30`. `0` is level; positive values put the right side ahead.
### `vortex_outward`
Direction of the `vortex` galaxy arms: `false` (default) sweeps in from the edges to the center, `true` bursts out from the center to the edges.
### `vortex_speed`
How fast the `vortex` galaxy runs, from `1` (Very Slow) to `5` (Very Fast). Default `3`.
It scales `duration` for the vortex only.
### `vortex_clockwise`
Spin direction of the `vortex` galaxy on screen: `true` (default) clockwise, `false` counter-clockwise.
### `explosion_count`
How many fireballs the `explosion` sets off, from `1` to `5`: Single (one big blast), Few (~3), Some (~6, default `3`), Many (~10), Barrage (~16). More blasts means smaller ones.
### `explosion_speed`
How fast the `explosion` runs, from `1` (Very Slow) to `5` (Very Fast). Default `3`.
It scales `duration` for the explosion only.
### `explosion_rhythm`
When the blasts go off: `0` Together, `1` Staggered (default, slight random delays), `2` Chain (a reaction sweeping left to right).
### `explosion_debris`
How many streaks and specks the blasts throw: `0` None, `1` Some (default), `2` Lots.

# Example config
Here's an example config, with every variable added (exept for pallete)
```
[color]
from = "#ff0000"
to = "#00ff00"

[animation]
fps              = 60
wind             = 0
height           = 1
direction        = false
duration         = 1
flames_duration  = 0.7
effect           = "fire"
wave_speed       = 3
wave_angle       = 0
vortex_outward   = false
vortex_speed     = 3
vortex_clockwise = true
explosion_count  = 3
explosion_speed  = 3
explosion_rhythm = 1
explosion_debris = 1
```
