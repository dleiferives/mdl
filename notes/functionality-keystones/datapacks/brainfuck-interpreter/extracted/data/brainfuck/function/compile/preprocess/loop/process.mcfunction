#execute if score #cur_char brainfuck.re matches 0..1 run say +-
#execute if score #cur_char brainfuck.re matches 2..3 run say <>

#tellraw @a [{text:"addsub: "},{score:{name:"#addsub",objective:"brainfuck.re"}}]

execute if score #cur_char brainfuck.re matches 0..1 run function brainfuck:compile/preprocess/fold/shift
execute if score #cur_char brainfuck.re matches 0..1 if score #addsub brainfuck.re matches 0 run scoreboard players set #addsub brainfuck.re 1
execute if score #cur_char brainfuck.re matches 0 run return run function brainfuck:compile/preprocess/loop/return/add
execute if score #cur_char brainfuck.re matches 1 run return run function brainfuck:compile/preprocess/loop/return/remove

execute if score #cur_char brainfuck.re matches 2..3 run function brainfuck:compile/preprocess/fold/add
execute if score #cur_char brainfuck.re matches 2 run return run function brainfuck:compile/preprocess/loop/return/left_shift
execute if score #cur_char brainfuck.re matches 3 run return run function brainfuck:compile/preprocess/loop/return/right_shift

function brainfuck:compile/preprocess/fold/shift
function brainfuck:compile/preprocess/fold/add
scoreboard players set #add_value brainfuck.re 0
scoreboard players set #shift_value brainfuck.re 0

execute if score #cur_char brainfuck.re matches 4 run return run function brainfuck:compile/preprocess/add_command/input
execute if score #cur_char brainfuck.re matches 5 run return run function brainfuck:compile/preprocess/add_command/output
execute if score #cur_char brainfuck.re matches 6 run return run function brainfuck:compile/preprocess/match/stack_push
execute if score #cur_char brainfuck.re matches 7 unless function brainfuck:compile/preprocess/match/stack_pop/ run return fail
execute if score #cur_char brainfuck.re matches 7 run return 1
##说明遇到了+-<>，+1以便下次loop进行folding尝试
#return run scoreboard players add #folding_count brainfuck.re 1