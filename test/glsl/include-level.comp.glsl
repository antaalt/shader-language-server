#version 450

#extension GL_GOOGLE_include_directive : require

#include "./inc0/level0.glsl"

void compute() {
    float level = level0;
    //vec4 frags = gl_FragCoord; // Error
    uvec3 o = gl_GlobalInvocationID;
    uint i = gl_LocalInvocationIndex;
}