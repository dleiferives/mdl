scoreboard players add #count brainfuck.re 1
scoreboard players add #count_c brainfuck.re 1

data remove storage brainfuck:re cur_char
function brainfuck:compile/convert/score_to_storage/ir_ip
function brainfuck:compile/get_char/_ with storage brainfuck:re
#由于80条命令一次性执行，会导致ir_ip越界，但是不影响运行

function brainfuck:compile/process/

scoreboard players add #ir_ip brainfuck.re 1

execute if score #count_c brainfuck.re > #max_single_command_count brainfuck.re run return fail
return 1