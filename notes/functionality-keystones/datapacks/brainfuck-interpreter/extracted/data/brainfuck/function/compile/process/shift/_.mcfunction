$execute store result storage brainfuck:re array[$(las_ptr)] int 1 run scoreboard players get #ptr_target brainfuck.re
scoreboard players set #sync brainfuck.re 0
$execute store result score #ptr_target brainfuck.re run data get storage brainfuck:re array[$(ptr)]