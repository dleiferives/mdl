function brainfuck:compile/convert/score_to_storage/ptr

data remove storage brainfuck:re ascii_tmp
data modify storage brainfuck:re ascii_tmp set string storage brainfuck:re input 0 1
data modify storage brainfuck:re input set string storage brainfuck:re input 1

#tellraw @a {storage:"brainfuck:re",nbt:"ascii_tmp"}

#如果不能找到对应的输入则终止程序
execute unless data storage brainfuck:re ascii_tmp run return run return run scoreboard players set #cmd_stop brainfuck.re 1
function brainfuck:compile/process/input/_