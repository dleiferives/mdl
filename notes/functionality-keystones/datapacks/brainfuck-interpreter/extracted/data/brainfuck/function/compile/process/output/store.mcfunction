#$execute store result score #tmp_value brainfuck.re run data get storage brainfuck:re array[$(ptr)]
scoreboard players operation #tmp_value brainfuck.re = #ptr_target brainfuck.re
function brainfuck:compile/convert/score_to_ascii