execute if score #array_length_c brainfuck.re matches 0 run return 1

execute if score #array_length_c brainfuck.re matches 2048.. run return run function brainfuck:init/array/2048
execute if score #array_length_c brainfuck.re matches 1024.. run return run function brainfuck:init/array/1024
execute if score #array_length_c brainfuck.re matches 512.. run return run function brainfuck:init/array/512
execute if score #array_length_c brainfuck.re matches 256.. run return run function brainfuck:init/array/256
execute if score #array_length_c brainfuck.re matches 128.. run return run function brainfuck:init/array/128
execute if score #array_length_c brainfuck.re matches 64.. run return run function brainfuck:init/array/64
execute if score #array_length_c brainfuck.re matches 32.. run return run function brainfuck:init/array/32
execute if score #array_length_c brainfuck.re matches 16.. run return run function brainfuck:init/array/16
execute if score #array_length_c brainfuck.re matches 8.. run return run function brainfuck:init/array/8
execute if score #array_length_c brainfuck.re matches 4.. run return run function brainfuck:init/array/4
execute if score #array_length_c brainfuck.re matches 2.. run return run function brainfuck:init/array/2
execute if score #array_length_c brainfuck.re matches 1.. run return run function brainfuck:init/array/1

#scoreboard players remove #array_length_c brainfuck.re 1
#data modify storage brainfuck:re array append value 0ub

#say 1

#function brainfuck:init/array/_