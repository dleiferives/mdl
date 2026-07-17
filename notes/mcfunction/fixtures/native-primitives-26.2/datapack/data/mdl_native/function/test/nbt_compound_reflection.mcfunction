# Research-only reproduction attempt of the idea behind Bookshelf bs.dump's
# compound-key reflection. Bookshelf credits tryashtar and PiggyPig. On the
# isolated 26.2 server this stores an unresolved NBT text component and does not
# produce the expected `extra` tree; the negative result is intentionally kept.
data remove storage mdl:observations nbt_reflection
execute in minecraft:overworld run forceload add 0 0
data modify storage mdl:observations nbt_reflection.work set value {alpha:1,"space key":2,"a.b":3,"a\"b":4}
execute in minecraft:overworld run setblock 1 100 0 minecraft:chest
execute in minecraft:overworld run loot replace block 1 100 0 container.0 loot {pools:[{rolls:1,entries:[{type:"item",name:"egg",functions:[{function:"set_name",entity:"this",name:{storage:"mdl:observations",nbt:"nbt_reflection.work"}}]}]}]}
execute in minecraft:overworld run data modify storage mdl:observations nbt_reflection.rendered set from block 1 100 0 Items[0].components.minecraft:custom_name
execute in minecraft:overworld store success storage mdl:observations nbt_reflection.extra_present byte 1 run data modify storage mdl:observations nbt_reflection.extra set from block 1 100 0 Items[0].components.minecraft:custom_name.extra

# Compound Key Reader used a sign as a data-to-text serialization boundary. The
# 1.21-era JSON-string form has changed, but a direct 26.2 NBT text component is
# resolved when written into a sign message and yields a structured token tree.
execute in minecraft:overworld run setblock 1 100 0 minecraft:oak_sign
execute in minecraft:overworld run data modify block 1 100 0 front_text.messages[0] set value {storage:"mdl:observations",nbt:"nbt_reflection.work"}
execute in minecraft:overworld run data modify storage mdl:observations nbt_reflection.sign_resolved set from block 1 100 0 front_text.messages[0]
execute in minecraft:overworld store success storage mdl:observations nbt_reflection.sign_extra_present byte 1 run data modify storage mdl:observations nbt_reflection.sign_extra set from block 1 100 0 front_text.messages[0].extra

execute in minecraft:overworld run setblock 1 100 0 air
execute in minecraft:overworld run forceload remove 0 0
data get storage mdl:observations nbt_reflection
