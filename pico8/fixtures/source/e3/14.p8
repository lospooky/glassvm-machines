pico-8 cartridge // http://www.pico-8.com
version 42
__lua__
-- lorenz attractor
-- by aury arthan

-- parameters
dt = 0.01 -- timestep
x0,y0,z0 = -11.075,-16.996,21.779 -- initial state
clrs = {1,2,3,8,9,15,10,11,12,4} -- color scale
pal(3,136,1);pal(9,137,1);pal(15,9,1);pal(4,140,1)
-- grid
grid = {}
for i=1,128 do
	grid[i] = {}
	for j=1,128 do
		grid[i][j] = 0
	end
end

function _init()
 xyz = {x0,y0,z0}
 x,z = gi(xyz)
 cls()
end

function _update60()
	xyz = rk4(xyz) -- integrate
	px,pz = x,z -- previous state
	x,z = gi(xyz) -- new state
	if inbounds(px,pz) then grid[px][pz] += 1	end -- update grid
end

function _draw()
	-- grid update
	if inbounds(px,pz) then	pset(px-1,pz-1,clrs[(grid[px][pz]-1)%(#clrs)+1]) end
	-- state point
	pset(x-1,z-1,7)
end

-- odes
function dx(xyz)
	return 10*(xyz[2]-xyz[1])
end
function dy(xyz)
	return xyz[1]*(28-xyz[3])-xyz[2]
end
function dz(xyz)
	return xyz[1]*xyz[2]-8/3*xyz[3]
end
function odes()
	return {dx,dy,dz}
end

-- integrator
function rk4(xyz)
	-- k1
	local k1 = {}
	for i=1,3 do
		add(k1,odes()[i](xyz)*dt)
	end
	-- k2
	local k2 = {}
	for i=1,3 do
		add(k2,odes()[i](sum(xyz,div(k1,2)))*dt)
	end
	-- k3
	local k3 = {}
	for i=1,3 do
		add(k3,odes()[i](sum(xyz,div(k2,2)))*dt)
	end
	-- k4
	local k4 = {}
	for i=1,3 do
		add(k4,odes()[i](sum(xyz,k3))*dt)
	end
	-- result
	local r = {}
	for i=1,3 do
		add(r,xyz[i]+(k1[i]+2*k2[i]+2*k3[i]+k4[i])/6)
	end
	return r
end

-- array operations
function sum(a,b)
	local r = {}
	for i=1,#a do
		add(r,a[i]+b[i])
	end
	return r
end
function div(a,d)
	local r = {}
	for i=1,#a do
		add(r,a[i]/d)
	end
	return r
end

-- grid index
xmin,xmax,zmin,zmax = -19,19,3,47
function gi(xyz)
	return flr((xyz[1]-xmin)*128/(xmax-xmin)),flr(128-(xyz[3]-zmin)*128/(zmax-zmin))
end
-- inbounds check
function inbounds(x,z)
	if x>0 and x<129 and z>0 and z<129 then
		return true
	end
	return false
end
__gfx__
00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00700700000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00077000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00077000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00700700000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
