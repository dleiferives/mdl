execute if score #shift_value brainfuck.re matches 0 run return 1
function brainfuck:compile/preprocess/add_command/shift
scoreboard players set #shift_value brainfuck.re 0
scoreboard players set #addsub brainfuck.re -1