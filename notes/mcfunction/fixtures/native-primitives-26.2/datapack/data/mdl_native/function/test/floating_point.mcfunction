data remove storage mdl:observations floats
scoreboard objectives remove mdl_float
scoreboard objectives add mdl_float dummy

# Literal storage exposes binary32 versus binary64 precision and signed zero.
data modify storage mdl:observations floats.literals set value {ordinary_f:1.5f,ordinary_d:1.5d,f24_boundary:16777217f,d53_boundary:9007199254740993d,negative_zero_f:-0.0f,negative_zero_d:-0.0d,scientific_f:1.25e3f,scientific_d:1.25e-3d}
data modify storage mdl:observations floats.zero.value set value -0.0f
execute store success storage mdl:observations floats.zero.replace_with_positive_success byte 1 run data modify storage mdl:observations floats.zero.value set value 0.0f

# `data get` first projects a numeric NBT value into the signed command-result int.
data modify storage mdl:observations floats.inputs set value {positive:1.9d,negative:-1.9d,scaled:1.25d,large:1e30d,tiny:1e-30d}
execute store result storage mdl:observations floats.data_get.positive int 1 run data get storage mdl:observations floats.inputs.positive
execute store result storage mdl:observations floats.data_get.negative int 1 run data get storage mdl:observations floats.inputs.negative
execute store result storage mdl:observations floats.data_get.scaled int 1 run data get storage mdl:observations floats.inputs.scaled 10
execute store result storage mdl:observations floats.data_get.large int 1 run data get storage mdl:observations floats.inputs.large
execute store result storage mdl:observations floats.data_get.tiny int 1 run data get storage mdl:observations floats.inputs.tiny

# `execute store` applies its double scale and casts into the requested NBT type.
scoreboard players set #positive mdl_float 7
scoreboard players set #negative mdl_float -7
scoreboard players set #f24 mdl_float 16777217
execute store result storage mdl:observations floats.execute_store.half_positive float 0.5 run scoreboard players get #positive mdl_float
execute store result storage mdl:observations floats.execute_store.half_negative float 0.5 run scoreboard players get #negative mdl_float
execute store result storage mdl:observations floats.execute_store.f24 float 1 run scoreboard players get #f24 mdl_float
execute store result storage mdl:observations floats.execute_store.d53 double 1 run scoreboard players get #f24 mdl_float
execute store result storage mdl:observations floats.execute_store.overflow float 100000000000000000000000000000000000000 run scoreboard players get #f24 mdl_float
execute store result storage mdl:observations floats.execute_store.underflow float 0.00000000000000000000000000000000000000000000000001 run scoreboard players get #positive mdl_float

# Macro substitution strips an input number's suffix, so a template can retype it.
function mdl_native:float/as_float {label:"from_double",value:1.25d}
function mdl_native:float/as_double {label:"from_float",value:1.25f}
function mdl_native:float/as_float {label:"precision",value:16777217d}

# Display transformation decomposition can act as a float arithmetic coprocessor.
execute in minecraft:overworld run forceload add 0 0
schedule function mdl_native:float/entity_spawn 1t replace

data get storage mdl:observations floats
scoreboard objectives remove mdl_float
