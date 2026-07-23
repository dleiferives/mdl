#execute if data storage brainfuck:re {stack:[]} run say empty
execute if data storage brainfuck:re {stack:[]} run return fail
#]匹配到的[一定是stack里的最后一个
function brainfuck:compile/convert/score_to_storage/ir_ip

execute store result score #match_pos brainfuck.re run data modify storage brainfuck:re match_pos set from storage brainfuck:re stack[-1]
data remove storage brainfuck:re stack[-1]
execute if score #addsub brainfuck.re matches 1 run return run function brainfuck:compile/preprocess/add_command/reset/

function brainfuck:compile/preprocess/match/stack_pop/set_jump_table with storage brainfuck:re
scoreboard players add #ir_ip brainfuck.re 1

return 1