data remove storage mdl:observations scoreboard
scoreboard objectives remove mdl_probe
scoreboard objectives add mdl_probe dummy
scoreboard players set #max mdl_probe 2147483647
scoreboard players add #max mdl_probe 1
execute store result storage mdl:observations scoreboard.max_plus_one int 1 run scoreboard players get #max mdl_probe
scoreboard players set #min mdl_probe -2147483648
scoreboard players remove #min mdl_probe 1
execute store result storage mdl:observations scoreboard.min_minus_one int 1 run scoreboard players get #min mdl_probe
scoreboard players set #left mdl_probe -7
scoreboard players set #right mdl_probe 3
scoreboard players operation #left mdl_probe /= #right mdl_probe
execute store result storage mdl:observations scoreboard.negative_division int 1 run scoreboard players get #left mdl_probe
scoreboard players set #left mdl_probe -7
scoreboard players operation #left mdl_probe %= #right mdl_probe
execute store result storage mdl:observations scoreboard.negative_remainder int 1 run scoreboard players get #left mdl_probe
scoreboard players reset #missing mdl_probe
scoreboard players set #left mdl_probe 12
scoreboard players operation #left mdl_probe = #missing mdl_probe
execute store result storage mdl:observations scoreboard.missing_source int 1 run scoreboard players get #left mdl_probe
scoreboard players set #left mdl_probe 9
scoreboard players set #right mdl_probe 0
execute store success storage mdl:observations scoreboard.divide_zero_success byte 1 store result storage mdl:observations scoreboard.divide_zero_result int 1 run scoreboard players operation #left mdl_probe /= #right mdl_probe
execute store result storage mdl:observations scoreboard.divide_zero_value int 1 run scoreboard players get #left mdl_probe
data get storage mdl:observations scoreboard
