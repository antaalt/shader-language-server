#include "inc0/level0.hlsl"

float4 fs_main(uint3 dtid : SV_DispatchThreadID) : SV_RenderTarget0 {
    
    return float4(level0,1,1,0);
}