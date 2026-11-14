pico-8 cartridge // http://www.pico-8.com
version 42
__lua__
function _init()
	poke(0x5f2c,3)

	
	
	cells = {}
	for i = 1, 64 do
		cells[i] = {}	
		for j=1, 64 do
			cells[i][j] = true
		end
		
	end
	
	for y = 1, 64 do
		for x = 1, 64 do
			if rnd() > 0.8 then
				cells[y][x] = true
			else
				cells[y][x] = false
			end
		end
	end
	
tempcells = {}
for y = 1, 64 do
	tempcells[y] = {}
	for x = 1,64 do
		tempcells[y][x] = cells[y][x]
	end
end
	fps = 0	

end

function _update()
fps += 1

if fps == 2 then
	sim()
	fps = 0
end
end

function sim()

	for y = 1, 64 do
		for x = 1, 64 do
			n = 0
			
			if y > 1 then
				if x > 1 then
					if cells[y-1][x-1] == true then
						n += 1
					end
				end
				if cells[y-1][x] == true then
					n += 1
				end
				if x < 64 then
					if cells[y-1][x+1] == true then
						n += 1
					end
				end
			end
			
			if y < 64 then
				if x > 1 then
					if cells[y+1][x-1] == true then
						n += 1
					end
				end
				if cells[y+1][x] == true then
					n += 1
				end
				if x < 64 then
					if cells[y+1][x+1] == true then
						n += 1
					end
				end
			end
			
			if x < 64 then
				if cells[y][x+1] == true then
					n += 1
				end
			end
			
			if x > 1 and cells[y][x-1] == true then
				n += 1
			end
			
			if cells[y][x] == true then
				if n == 2 or n == 3 then
					tempcells[y][x] = true	
				
				else
					tempcells[y][x] = false
				end
			else
				if n == 3 then
				tempcells[y][x] = true
				end
		end
	end
end

	for y = 1,64 do
		for x = 1,64 do
			cells[y][x] = tempcells[y][x]
		end
	end
end

function _draw()
	cls()
	
	for y = 1, 64 do
		for x = 1,64 do
			if cells[y][x] == true then
				pset(x, y, 11)
			else
				pset(x,y, 0)
			end
		end
	end
end
__gfx__
00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00700700000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00077000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00077000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00700700000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
