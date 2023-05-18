This repo reproduces a bug that I found on Intel Arc A770 GPU Windows driver.

The Intel driver declares a `shaderGroupBaseAlignment` of 32 in [VkPhysicalDeviceRayTracingPipelinePropertiesKHR](https://registry.khronos.org/vulkan/specs/1.3-extensions/man/html/VkPhysicalDeviceRayTracingPipelinePropertiesKHR.html).

However, when I 32-byte align the SBT records for the ray generation shader, the data read from the SBT record becomes garbage.

Once I manually override the value of `shaderGroupBaseAlignment` to be 64, everything works as expected again.

To run this demo, simply install (Rust)[https://rustup.rs/]  and type `cargo run` in your terminal. A message "Test passed!" should be printed onto the screen.

Now, change the `BASE_OFFSET` constant on line 14 of `main.rs` to be 32. The assertion on the data read back from the SBT records fails.

It's not clear what has caused this bug. Intel can fix this by simply annoucing `shaderGroupBaseAlignment = 64` in `VkPhysicalDeviceRayTracingPipelinePropertiesKHR`, but it would be preferred if Intel can root-cause the problem.
