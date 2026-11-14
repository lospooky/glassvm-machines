pico-8 cartridge // http://www.pico-8.com
version 42
__lua__



function vec3(_x,_y,_z)
	return{
	x=_x,
	y=_y,
	z=_z
	}
end
function make_tri(vecs)
	local _tri={}
	_tri[1]=vecs[1]
	_tri[2]=vecs[2]
	_tri[3]=vecs[3]
	return _tri
end

function make_mesh(list)
	local _mesh={}
 	for k,v in ipairs(list) do
 		add(_mesh,v)
 	end
 	return _mesh
end

function multiply_vec(_vec, result, m)
    result.x = _vec.x * m[0][0] + _vec.y * m[1][0] + _vec.z * m[2][0] + m[3][0]
    result.y = _vec.x * m[0][1] + _vec.y * m[1][1] + _vec.z * m[2][1] + m[3][1]
    result.z = _vec.x * m[0][2] + _vec.y * m[1][2] + _vec.z * m[2][2] + m[3][2]
    local w = _vec.x * m[0][3] + _vec.y * m[1][3] + _vec.z * m[2][3] + m[3][3]
    if w ~= 0.0 then
        result.x = result.x / w
        result.y = result.y / w
        result.z = result.z / w
    end
end
function make_matriz(_mat)
	for i=0,3 do
		_mat[i]={}
		for ii=0,3 do
		_mat[i][ii]=0.0
		end
	end	
end

function _init()
	t=0
	cube=make_mesh(
	{
	--south
	make_tri({ vec3(0.0,0.0,0.0), vec3(0.0,1.0,0.0), vec3(1.0,1.0,0.0)}),
	make_tri({ vec3(0.0,0.0,0.0), vec3(1.0,1.0,0.0), vec3(1.0,0.0,0.0)}),
	--east
	make_tri({ vec3(1.0,0.0,0.0), vec3(1.0,1.0,0.0), vec3(1.0,1.0,1.0)}),
	make_tri({ vec3(1.0,0.0,0.0), vec3(1.0,1.0,1.0), vec3(1.0,0.0,1.0)}),
	--north
	make_tri({ vec3(1.0,0.0,1.0), vec3(1.0,1.0,1.0), vec3(0.0,1.0,1.0)}),
	make_tri({ vec3(1.0,0.0,1.0), vec3(0.0,1.0,1.0), vec3(0.0,0.0,1.0)}),
	--west
	make_tri({ vec3(0.0,0.0,1.0), vec3(0.0,1.0,1.0), vec3(0.0,1.0,0.0)}),
	make_tri({ vec3(0.0,0.0,1.0), vec3(0.0,1.0,0.0), vec3(0.0,0.0,0.0)}),
	--top
	make_tri({ vec3(0.0,1.0,0.0), vec3(0.0,1.0,1.0), vec3(1.0,1.0,1.0)}),
	make_tri({ vec3(0.0,1.0,0.0), vec3(1.0,1.0,1.0), vec3(1.0,1.0,0.0)}),
	--bottom
	make_tri({ vec3(1.0,0.0,1.0), vec3(0.0,0.0,1.0), vec3(0.0,0.0,0.0)}),
   	make_tri({ vec3(1.0, 0.0, 1.0), vec3(0.0, 0.0, 0.0), vec3(1.0, 0.0, 0.0) }),
	}
	)
	

	near=1.0
	far=1000.0
	fov=90.0
	aspect_ratio=128/128
	fov_rad=1.000001
	
	cam=vec3(0.0,0.0,0.0)
	m_proj={}
	make_matriz(m_proj)
	
	m_rot_x={}
	m_rot_z={}
	make_matriz(m_rot_x)
	make_matriz(m_rot_z)
	
	
	m_proj[0][0]=aspect_ratio*fov_rad
	m_proj[1][1]=fov_rad
	m_proj[2][2]=far/(far-near)
	m_proj[3][2]=(-far*near)/(far-near)
	m_proj[2][3]=1.0
	m_proj[3][3]=0.0
	
end

function clone_tri(_tri)
	return make_tri(_tri)
end


function get_colo(lum)
 local pixel=flr(13*lum)
 if pixel<=3 then
  return 1
 elseif pixel<=4 then
  return 1
 elseif pixel<=6 then
  return 2
 elseif pixel<=7 then
 	return 2
 elseif pixel<=8 then
  return 8
 elseif pixel<=12 then
  return 8
 end
 
 
 	
  return 0
end

function _draw()

	cls()
	
	for k,v in ipairs(cube) do
		local proj=make_tri({vec3(0.0,0.0,0.0),vec3(0.0,0.0,0.0),vec3(0.0,0.0,0.0)})
		
		local r_z=make_tri({vec3(0.0,0.0,0.0),vec3(0.0,0.0,0.0),vec3(0.0,0.0,0.0)})
		local r_zx=make_tri({vec3(0.0,0.0,0.0),vec3(0.0,0.0,0.0),vec3(0.0,0.0,0.0)})
	
		
		multiply_vec(v[1],r_z[1],m_rot_z)
		multiply_vec(v[2],r_z[2],m_rot_z)
		multiply_vec(v[3],r_z[3],m_rot_z)
		
		multiply_vec(r_z[1],r_zx[1],m_rot_x)
		multiply_vec(r_z[2],r_zx[2],m_rot_x)
		multiply_vec(r_z[3],r_zx[3],m_rot_x)
	
		


      local trans=r_zx
	   trans[1].z=r_zx[1].z+3.0
	   trans[2].z=r_zx[2].z+3.0
	   trans[3].z=r_zx[3].z+3.0
		
		local normal,line1,line2=vec3(0.0,0.0,0.0),vec3(0.0,0.0,0.0),vec3(0.0,0.0,0.0)	
		
		line1.x = trans[2].x - trans[1].x
		line1.y = trans[2].y - trans[1].y
		line1.z = trans[2].z - trans[1].z
		
	 line2.x = trans[3].x - trans[1].x 
  line2.y = trans[3].y - trans[1].y 
  line2.z = trans[3].z - trans[1].z
		
		normal.x=line1.y*line2.z-line1.z*line2.y					
		normal.y=line1.z*line2.x-line1.x*line2.z
		normal.z=line1.x*line2.y-line1.y*line2.x
		
		local l = sqrt((normal.x * normal.x) + (normal.y * normal.y) + (normal.z * normal.z))
			
			normal.x = normal.x / l
			normal.y = normal.y / l
			normal.z = normal.z / l


		local dot = normal.x * (trans[1].x - cam.x) + normal.y * (trans[1].y - cam.y) + normal.z * (trans[1].z - cam.z)


		if dot < 0.0 then
   
   light=vec3(0.0,0.0,-1.0)
		 local l = sqrt((light.x * light.x) + (light.y * light.y) + (light.z * light.z))
		 light.x=light.x/l
			light.y=light.y/l
			light.z=light.z/l
			
   local dp=normal.x*light.x+normal.y*light.y+normal.z*light.z
  
   
			multiply_vec(trans[1],proj[1],m_proj)
			multiply_vec(trans[2],proj[2],m_proj)
			multiply_vec(trans[3],proj[3],m_proj)	
			proj[1].x=proj[1].x+1.0
			proj[1].y=proj[1].y+1.0			
			proj[2].x=proj[2].x+1.0
			proj[2].y=proj[2].y+1.0				
			proj[3].x=proj[3].x+1.0
			proj[3].y=proj[3].y+1.0			
			proj[1].x=proj[1].x*0.5*128
			proj[1].y=proj[1].y*0.5*128			
			proj[2].x=proj[2].x*0.5*128
			proj[2].y=proj[2].y*0.5*128		
			proj[3].x=proj[3].x*0.5*128
			proj[3].y=proj[3].y*0.5*128		
			
			draw_tri(proj[1].x,proj[1].y,proj[2].x,proj[2].y,proj[3].x,proj[3].y,get_colo(dp))
		end
	end
end

function _update()
   
	t=t+1.0/100
	m_rot_z[0][0]=cos(t)
	m_rot_z[0][1]=sin(t)
	m_rot_z[1][0]=-sin(t)
	m_rot_z[1][1]=cos(t)
	m_rot_z[2][2]=1.0
	m_rot_z[3][3]=1.0
	m_rot_x[0][0]=1.0
	m_rot_x[1][1]=cos(t*0.5)
	m_rot_x[1][2]=sin(t*0.5)
	m_rot_x[2][1]=-sin(t*0.5)		
	m_rot_x[2][2]=cos(t*0.5)
	m_rot_x[3][3]=1.0	
end

function draw_tri(_x1,_y1,_x2,_y2,_x3,_y3,colo)
	line(_x1,_y1,_x2,_y2,colo)
	line(_x2,_y2,_x3,_y3,colo)
	line(_x3,_y3,_x1,_y1,colo)
	
end
__gfx__
00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00700700000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00077000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00077000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00700700000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
