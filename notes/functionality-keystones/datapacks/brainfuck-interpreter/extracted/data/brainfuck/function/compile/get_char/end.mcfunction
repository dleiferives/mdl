function brainfuck:compile/process/output/start
execute if score #cmd_stop brainfuck.re matches 1 run tellraw @a {text:"The program is stopped.",color:"yellow"}
execute if score #cmd_stop brainfuck.re matches 0 run tellraw @a {text:"The program is completed.",color:"yellow"}
#临时输出执行次数
tellraw @a [{text:"Execution count(s): ",color:"green"},{score:{name:"#count",objective:"brainfuck.re"}},{text:"; Execution time: "},{score:{name:"#stopwatch",objective:"brainfuck.re"}},{text:" tick(s)"}]
scoreboard players set #running brainfuck.re 0
scoreboard players set #cmd_stop brainfuck.re 0
schedule clear brainfuck:compile/get_char/