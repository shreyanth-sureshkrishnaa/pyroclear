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
`animation` has 6 valid variables, `fps`, `wind`, `height`, `direction`, `duration`, and `flames_duration`

| Name | Type | Example |
| :--- | :--- | :--- |
| `fps` | integer | `60` |
| `wind` | integer | `0` |
| `height` | integer | `1` |
| `direction` | boolean | `false` |
| `duration` | float | `1.0` |
| `flames_duration` |  float | `0.7` |

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
```
