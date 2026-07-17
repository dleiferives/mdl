data remove storage mdl:observations nbt
data modify storage mdl:observations nbt.types set value {byte:1b,short:2s,int:3,long:4L,float:1.5f,double:2.5d,string:"hé🙂",bytes:[B;-128b,0b,127b],ints:[I;-2147483648,0,2147483647],longs:[L;-9223372036854775808L,0L,9223372036854775807L],hetero:[1b,"two",{three:3},[4,5]],dict:{alpha:1,"space key":2,nested:{x:3}}}
execute store result storage mdl:observations nbt.string_units int 1 run data get storage mdl:observations nbt.types.string
execute store result storage mdl:observations nbt.list_length int 1 run data get storage mdl:observations nbt.types.hetero
execute store result storage mdl:observations nbt.compound_size int 1 run data get storage mdl:observations nbt.types.dict
data modify storage mdl:observations nbt.list_probe set value {xs:[10,20,30],batch:[40,50],groups:[{id:"a",xs:[1]},{id:"b",xs:[2]}]}
data modify storage mdl:observations nbt.list_probe.xs insert 0 value 5
data modify storage mdl:observations nbt.list_probe.xs insert -1 value 25
data remove storage mdl:observations nbt.list_probe.xs[-1]
data modify storage mdl:observations nbt.list_probe.xs append from storage mdl:observations nbt.list_probe.batch[]
data modify storage mdl:observations nbt.list_probe.groups[].xs append from storage mdl:observations nbt.list_probe.batch[]
data modify storage mdl:observations nbt.self_append set value [1,2,3]
data modify storage mdl:observations nbt.self_append append from storage mdl:observations nbt.self_append[]
data modify storage mdl:observations nbt.merge_left set value {a:1,nested:{x:1,y:2}}
data modify storage mdl:observations nbt.merge_left merge value {b:2,nested:{y:9,z:3}}
data modify storage mdl:observations nbt.slice set string storage mdl:observations nbt.types.string 2 4
data get storage mdl:observations nbt
