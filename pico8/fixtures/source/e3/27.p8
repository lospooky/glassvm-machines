pico-8 cartridge // http://www.pico-8.com
version 42
__lua__
-- 3d attempt!
--  by bp103


-- (i know its not voxels)
voxl={}
function vox(x3d,y3d,z3d)
	local zoom = 128
	local depth = 40
	zdp = z3d+depth
	voxl.x = zoom*(x3d/zdp)+64
	voxl.y = zoom*(y3d/zdp)+64
end


function _init()
end

--genaric array
a={}
function _draw()
	cls(0)
	
	xg=(sin(i)*120)-60
	yg=(cos(i)*120)
	
	xr=(cos(i)*40)+xg
	yr=(sin(i)*40)+yg
	
	zhov = (sin(q)*180)+120
	
	step = 16
	for x=-128,208,step do
		for y=-128,208,step do
			vox(x+xr,y+yr,128+zhov)
			a.x = voxl.x
			a.y = voxl.y
			vox((x+step)+xr,(y+step)+yr,128+zhov)
			rectfill(a.x,a.y,voxl.x,voxl.y,3)
			rect(a.x,a.y,voxl.x,voxl.y,1)
		end
	end
	
	for z=127,16,(0-4) do
		for j=0,128,128 do
			vox((xr-32)+j,yr-32,z+zhov)
			a.x = voxl.x
			a.y = voxl.y
			vox((xr+16)+j,yr+16,z+zhov)
			rectfill(a.x,a.y,voxl.x,voxl.y,(z%3)+5)
		end
		
		if z>96 then
			for j=0,256,64 do
				vox((xr-64)+j,yr-64,z+zhov)
				a.x = voxl.x
				a.y = voxl.y
				vox((xr-96)+j,yr-96,z+zhov)
				rectfill(a.x,a.y,voxl.x,voxl.y,(z%3)+5)
			end
		end
		
		if z>118 then
			for j=-112,80,16 do
				vox((xr+40)-j,yr+40,z+zhov)
				a.x = voxl.x
				a.y = voxl.y
				vox((xr+32)-j,yr+32,z+zhov)
				rectfill(a.x,a.y,voxl.x,voxl.y,(z%3)+5)
			end
		end		
	end		
end
q=0
i=0
function _update()
	i=i+0.005
	q=q+0.003
end
__sfx__
01100000274253342536425394253a425394253642533425274253342536425394253a425394253642533425264253242535425394253b425394253542532425264253242535425394253b425394253542532425
01100000031250f12503144031450f145000000314500000031450f14500000031450f145000000314503145021450e14502144021450e145000000214500000021450e145000000e14502145000000214502145
011000000e743346152861528615286432861534615286150e743286152861534615286433461500000346150e743346152861528615286433461534615346150e74334615286152861528643233131c3131a313
011000000e043286153461534615286433461528615286150e043286153461534615286433461500000286150e043286153461534615286433461528615286150e04328615346153461528643346150000028615
01100000194451b4451b405194451b4451e445204451e445194052144500000204451e445000001b44500000154451a44500000154451a4451d4451f4451d445000002044521445000001d445000001a44500000
__music__
00 00010204
02 00010304
