data remove storage brainfuck:re code.tmp_char
data modify storage brainfuck:re code.tmp_char set string storage brainfuck:re code.string 0 1
data modify storage brainfuck:re code.string set string storage brainfuck:re code.string 1

#tellraw @a {storage:"brainfuck:re",nbt:"code.tmp_char"}

#表示转换完了


execute unless data storage brainfuck:re code.tmp_char run return run function brainfuck:compile/preprocess/loop/end



function brainfuck:compile/preprocess/loop/char_to_score

execute if score #cur_char brainfuck.re matches -1 run return run function brainfuck:compile/preprocess/loop/

#tellraw @a [{score:{name:"#last_char",objective:"brainfuck.re"}},{text:" "},{score:{name:"#cur_char",objective:"brainfuck.re"}}]

#预处理
#execute \
unless score #last_char brainfuck.re matches -100 \
if score #last_char brainfuck.re matches 0..3 \
unless score #last_char brainfuck.re = #cur_char brainfuck.re \
run function brainfuck:compile/preprocess/loop/add_command/folding



#data modify storage brainfuck:re jump_table append value 0
execute unless function brainfuck:compile/preprocess/loop/process run return fail

#tellraw @a [{text:"add: "},{score:{name:"#add_value",objective:"brainfuck.re"}},\
{text:", shift: "},{score:{name:"#shift_value",objective:"brainfuck.re"}}\
]

#say next

#code_length++;
#scoreboard players add #code_length brainfuck.re 1

#execute unless data storage brainfuck:re code.tmp_char run say no

scoreboard players operation #last_char brainfuck.re = #cur_char brainfuck.re
scoreboard players add #ip brainfuck.re 1

#tellraw @a [{storage:"brainfuck:re",nbt:"code.list[-1]"}]



return run function brainfuck:compile/preprocess/loop/