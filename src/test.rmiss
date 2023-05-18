#version 460
layout(set = 0, binding = 1) buffer outputBuffer {
    int data[];
};

void main() {
    data[0] = 125;
}
