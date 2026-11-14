pico-8 cartridge // http://www.pico-8.com
version 42
__lua__
-- polyspiral
-- neko250

function _init()
	incr=0
end

function _update()
	incr=(incr+0.0005)%360
	x1=64
	y1=64
	length=1
	angle=incr
end

function _draw()
	cls()
	for i=1,150 do
		x2=x1+cos(angle)*length
		y2=y1+sin(angle)*length
		line(x1,y1,x2,y2,7)
		x1=x2
		y1=y2
		length=length+1
		angle=(angle+incr)%360
	end
end
