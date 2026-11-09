pico-8 cartridge // http://www.pico-8.com
version 42
__lua__
function _init()
 n=5
 v=1
 t=0
 p={}
 l=128
 
 function a(_x,_y,len,num)
  if num>1 then
   d(_x,_y+len/2,len/2,num-1)
   a(_x,_y,len/2,num-1)
   a(_x+len/2,_y,len/2,num-1)
   b(_x+len/2,_y+len/2,
     len/2,num-1)
  elseif num==1 then
   p[#p+1]=
   {x=_x+len/4,y=_y+3*len/4}
   p[#p+1]=
   {x=_x+len/4,y=_y+len/4}
   p[#p+1]=
   {x=_x+3*len/4,y=_y+len/4}
   p[#p+1]=
   {x=_x+3*len/4,y=_y+3*len/4}
  end
 end
 
 function b(_x,_y,len,num)
  if num>1 then
   c(_x+len/2,_y,len/2,num-1)
   b(_x,_y,len/2,num-1)
   b(_x,_y+len/2,len/2,num-1)
   a(_x+len/2,_y+len/2,
     len/2,num-1)
  elseif num==1 then
   p[#p+1]=
   {x=_x+3*len/4,y=_y+len/4}
   p[#p+1]=
   {x=_x+len/4,y=_y+len/4}
   p[#p+1]=
   {x=_x+len/4,y=_y+3*len/4}
   p[#p+1]=
   {x=_x+3*len/4,y=_y+3*len/4}
  end
 end
 
 function c(_x,_y,len,num)
  if num>1 then
   b(_x+len/2,_y,len/2,num-1)
   c(_x+len/2,_y+len/2,
     len/2,num-1)
   c(_x,_y+len/2,len/2,num-1)
   d(_x,_y,len/2,num-1)
  elseif num==1 then
   p[#p+1]=
   {x=_x+3*len/4,y=_y+len/4}
   p[#p+1]=
   {x=_x+3*len/4,y=_y+3*len/4}
   p[#p+1]=
   {x=_x+len/4,y=_y+3*len/4}
   p[#p+1]=
   {x=_x+len/4,y=_y+len/4}
  end
 end
 
 function d(_x,_y,len,num)
  if num>1 then
   a(_x,_y+len/2,len/2,num-1)
   d(_x+len/2,_y+len/2,
     len/2,num-1)
   d(_x+len/2,_y,len/2,num-1)
   c(_x,_y,len/2,num-1)
  elseif num==1 then
   p[#p+1]=
   {x=_x+len/4,y=_y+3*len/4}
   p[#p+1]=
   {x=_x+3*len/4,y=_y+3*len/4}
   p[#p+1]=
   {x=_x+3*len/4,y=_y+len/4}
   p[#p+1]=
   {x=_x+len/4,y=_y+len/4}
  end
 end
 
 a(0,0,l,n)
end

function _update60()
 t=(t+1)%2
 if t==0 then
  v=v+1
 end
end

function _draw()
 cls()
 for i=1,min(v,#p)-1 do
  line
  (p[i].x,p[i].y,
   p[i+1].x,p[i+1].y,7)
 end
end
__gfx__
00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00700700000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00077000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00077000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00700700000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
