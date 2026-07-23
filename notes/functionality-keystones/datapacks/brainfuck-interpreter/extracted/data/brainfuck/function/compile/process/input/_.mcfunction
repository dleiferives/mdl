execute store result score #ptr_target brainfuck.re run function brainfuck:compile/convert/ascii_to_score
scoreboard players set #sync brainfuck.re 1
#$execute store result storage brainfuck:re array[$(ptr)] int 1 run function brainfuck:compile/convert/ascii_to_score