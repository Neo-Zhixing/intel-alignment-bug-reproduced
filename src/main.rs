use std::{ffi::c_void, println, assert_eq};

use ash::vk;

fn main() {
    unsafe {
        run();
    }
}

/// On my Intel Arc A770 16GB, test passes with BASE_OFFSET = 64, but fails with BASE_OFFSET = 32 or 96.
/// Incorrect reads are observed in the SBT.
/// !!!! Change this into 32 to observe the bug !!!!
const BASE_OFFSET: usize = 64;

// SBT Layout:
// |             |      Raygen          |      Raymiss     |     Hitgroup     |
// | BASE_OFFSET |32|-SBT Data 64 bytes-|-32-|   Not used  |-32-|   Not used  |
//                    ^^^ Incorrect read here

unsafe fn create_buffer(
    device: &ash::Device,
    memory_type_index: u32,
    size: u64,
    usage: vk::BufferUsageFlags,
) -> (vk::Buffer, vk::DeviceMemory) {
    let buf = device
        .create_buffer(
            &vk::BufferCreateInfo {
                size,
                usage,
                ..Default::default()
            },
            None,
        )
        .unwrap();

    let requirements = device.get_buffer_memory_requirements(buf);
    assert!(requirements.memory_type_bits & (1 << memory_type_index) != 0);

    let flags = vk::MemoryAllocateFlagsInfo {
        flags: vk::MemoryAllocateFlags::DEVICE_ADDRESS,
        ..Default::default()
    };
    let mem = device
        .allocate_memory(
            &vk::MemoryAllocateInfo {
                allocation_size: requirements.size,
                memory_type_index,
                p_next: &flags as *const _ as *const _,
                ..Default::default()
            },
            None,
        )
        .unwrap();
    device.bind_buffer_memory(buf, mem, 0).unwrap();
    (buf, mem)
}

unsafe fn run() {
    let entry = ash::Entry::load().unwrap();
    let instance = entry
        .create_instance(
            &vk::InstanceCreateInfo {
                p_application_info: &vk::ApplicationInfo {
                    api_version: vk::make_api_version(0, 1, 3, 0),
                    ..Default::default()
                },
                ..Default::default()
            },
            None,
        )
        .unwrap();
    let pdevice = instance.enumerate_physical_devices().unwrap()[0];
    let pdevice_properties = instance.get_physical_device_properties(pdevice);
    let device_name = std::ffi::CStr::from_ptr(pdevice_properties.device_name.as_ptr() as _);
    println!("Using device: {}", device_name.to_str().unwrap());


    let mut rtx_features = vk::PhysicalDeviceRayTracingPipelineFeaturesKHR {
        ray_tracing_pipeline: vk::TRUE,
        ..Default::default()
    };
    let mut accel_struct_features = vk::PhysicalDeviceAccelerationStructureFeaturesKHR {
        acceleration_structure: vk::TRUE,
        ..Default::default()
    };
    let mut v12_features = vk::PhysicalDeviceVulkan12Features {
        buffer_device_address: vk::TRUE,
        ..Default::default()
    };
    let mut v13_features = vk::PhysicalDeviceVulkan13Features {
        synchronization2: vk::TRUE,
        ..Default::default()
    };
    let features = vk::PhysicalDeviceFeatures2::builder()
        .push_next(&mut rtx_features)
        .push_next(&mut accel_struct_features)
        .push_next(&mut v12_features)
        .push_next(&mut v13_features)
        .build();
    let device = instance
        .create_device(
            pdevice,
            &vk::DeviceCreateInfo {
                p_next: &features as *const _ as *const _,
                queue_create_info_count: 1,
                p_queue_create_infos: &vk::DeviceQueueCreateInfo {
                    queue_family_index: 0,
                    queue_count: 1,
                    p_queue_priorities: &1.0,
                    ..Default::default()
                },
                enabled_extension_count: 3,
                pp_enabled_extension_names: [
                    ash::extensions::khr::AccelerationStructure::name().as_ptr(),
                    ash::extensions::khr::RayTracingPipeline::name().as_ptr(),
                    ash::extensions::khr::DeferredHostOperations::name().as_ptr(),
                ]
                .as_slice()
                .as_ptr(),
                ..Default::default()
            },
            None,
        )
        .unwrap();

    let aabbs = vk::AabbPositionsKHR {
        min_x: 0.0,
        min_y: 0.0,
        min_z: 0.0,
        max_x: 1.0,
        max_y: 1.0,
        max_z: 1.0,
    };

    let mem_properties = instance.get_physical_device_memory_properties(pdevice);
    let (memory_type_index, _) = mem_properties
        .memory_types
        .iter()
        .take(mem_properties.memory_type_count as usize)
        .enumerate()
        .find(|(_, a)| {
            a.property_flags.contains(
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::DEVICE_LOCAL,
            )
        })
        .unwrap();
    let memory_type_index = memory_type_index as u32;

    let (blas_input_buf, blas_input_mem) = create_buffer(
        &device,
        memory_type_index,
        std::mem::size_of_val(&aabbs) as u64,
        vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR
            | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
    );
    {
        // Write input
        let ptr = device
            .map_memory(
                blas_input_mem,
                0,
                std::mem::size_of_val(&aabbs) as u64,
                Default::default(),
            )
            .unwrap();
        std::ptr::copy_nonoverlapping(&aabbs, ptr as *mut vk::AabbPositionsKHR, 1);
        device.unmap_memory(blas_input_mem);
    }

    let accel_struct_loader = ash::extensions::khr::AccelerationStructure::new(&instance, &device);

    // Create BLAS
    let blas_build_sizes = accel_struct_loader.get_acceleration_structure_build_sizes(
        vk::AccelerationStructureBuildTypeKHR::DEVICE,
        &vk::AccelerationStructureBuildGeometryInfoKHR {
            ty: vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL,
            flags: vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE,
            geometry_count: 1,
            p_geometries: [vk::AccelerationStructureGeometryKHR {
                geometry_type: vk::GeometryTypeKHR::AABBS,
                flags: vk::GeometryFlagsKHR::OPAQUE,
                geometry: vk::AccelerationStructureGeometryDataKHR {
                    aabbs: vk::AccelerationStructureGeometryAabbsDataKHR {
                        ..Default::default()
                    },
                },
                ..Default::default()
            }]
            .as_slice()
            .as_ptr(),
            ..Default::default()
        },
        &[1],
    );
    let (blas_backing_buf, _) = create_buffer(
        &device,
        memory_type_index,
        blas_build_sizes.acceleration_structure_size,
        vk::BufferUsageFlags::ACCELERATION_STRUCTURE_STORAGE_KHR,
    );

    let blas = accel_struct_loader
        .create_acceleration_structure(
            &vk::AccelerationStructureCreateInfoKHR {
                buffer: blas_backing_buf,
                offset: 0,
                size: blas_build_sizes.acceleration_structure_size,
                ty: vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL,
                ..Default::default()
            },
            None,
        )
        .unwrap();

    // Create TLAS
    let tlas_build_sizes = accel_struct_loader.get_acceleration_structure_build_sizes(
        vk::AccelerationStructureBuildTypeKHR::DEVICE,
        &vk::AccelerationStructureBuildGeometryInfoKHR {
            ty: vk::AccelerationStructureTypeKHR::TOP_LEVEL,
            flags: vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE,
            geometry_count: 1,
            p_geometries: [vk::AccelerationStructureGeometryKHR {
                geometry_type: vk::GeometryTypeKHR::INSTANCES,
                flags: vk::GeometryFlagsKHR::OPAQUE,
                geometry: vk::AccelerationStructureGeometryDataKHR {
                    instances: vk::AccelerationStructureGeometryInstancesDataKHR {
                        ..Default::default()
                    },
                },
                ..Default::default()
            }]
            .as_slice()
            .as_ptr(),
            ..Default::default()
        },
        &[1],
    );
    let (tlas_backing_buf, _) = create_buffer(
        &device,
        memory_type_index,
        tlas_build_sizes.acceleration_structure_size,
        vk::BufferUsageFlags::ACCELERATION_STRUCTURE_STORAGE_KHR,
    );

    let tlas = accel_struct_loader
        .create_acceleration_structure(
            &vk::AccelerationStructureCreateInfoKHR {
                buffer: tlas_backing_buf,
                offset: 0,
                size: tlas_build_sizes.acceleration_structure_size,
                ty: vk::AccelerationStructureTypeKHR::TOP_LEVEL,
                ..Default::default()
            },
            None,
        )
        .unwrap();

    let instances: vk::AccelerationStructureInstanceKHR = vk::AccelerationStructureInstanceKHR {
        transform: vk::TransformMatrixKHR {
            matrix: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        },
        instance_custom_index_and_mask: vk::Packed24_8::new(0, u8::MAX),
        instance_shader_binding_table_record_offset_and_flags: vk::Packed24_8::new(0, 0),
        acceleration_structure_reference: vk::AccelerationStructureReferenceKHR {
            device_handle: accel_struct_loader.get_acceleration_structure_device_address(
                &vk::AccelerationStructureDeviceAddressInfoKHR {
                    acceleration_structure: blas,
                    ..Default::default()
                },
            ),
        },
    };
    let (tlas_input_buf, tlas_input_mem) = create_buffer(
        &device,
        memory_type_index,
        tlas_build_sizes.acceleration_structure_size,
        vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR
            | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
    );
    {
        // Write input
        let ptr = device
            .map_memory(
                tlas_input_mem,
                0,
                std::mem::size_of_val(&instances) as u64,
                Default::default(),
            )
            .unwrap();
        std::ptr::copy_nonoverlapping(&instances, ptr as *mut vk::AccelerationStructureInstanceKHR, 1);
        device.unmap_memory(tlas_input_mem);
    }

    let (scratch_buf, _) = create_buffer(
        &device,
        memory_type_index,
        tlas_build_sizes
            .build_scratch_size
            .max(blas_build_sizes.build_scratch_size),
        vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
    );

    let command_pool = device
        .create_command_pool(&Default::default(), None)
        .unwrap();
    let command_buffer = device
        .allocate_command_buffers(&vk::CommandBufferAllocateInfo {
            command_pool,
            level: vk::CommandBufferLevel::PRIMARY,
            command_buffer_count: 1,
            ..Default::default()
        })
        .unwrap()[0];

    device
        .begin_command_buffer(command_buffer, &Default::default())
        .unwrap();
    accel_struct_loader.cmd_build_acceleration_structures(
        command_buffer,
        &[vk::AccelerationStructureBuildGeometryInfoKHR {
            ty: vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL,
            flags: vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE,
            mode: vk::BuildAccelerationStructureModeKHR::BUILD,
            dst_acceleration_structure: blas,
            geometry_count: 1,
            p_geometries: &vk::AccelerationStructureGeometryKHR {
                geometry_type: vk::GeometryTypeKHR::AABBS,
                geometry: vk::AccelerationStructureGeometryDataKHR {
                    aabbs: vk::AccelerationStructureGeometryAabbsDataKHR {
                        data: vk::DeviceOrHostAddressConstKHR {
                            device_address: device.get_buffer_device_address(
                                &vk::BufferDeviceAddressInfo {
                                    buffer: blas_input_buf,
                                    ..Default::default()
                                },
                            ),
                        },
                        stride: std::mem::size_of_val(&aabbs) as u64,
                        ..Default::default()
                    },
                },
                flags: vk::GeometryFlagsKHR::OPAQUE,
                ..Default::default()
            },
            scratch_data: vk::DeviceOrHostAddressKHR {
                device_address: device.get_buffer_device_address(&vk::BufferDeviceAddressInfo {
                    buffer: scratch_buf,
                    ..Default::default()
                }),
            },
            ..Default::default()
        }],
        &[&[vk::AccelerationStructureBuildRangeInfoKHR {
            primitive_count: 1,
            primitive_offset: 0,
            first_vertex: 0,
            transform_offset: 0,
        }]],
    );
    device.cmd_pipeline_barrier2(
        command_buffer,
        &vk::DependencyInfo {
            memory_barrier_count: 1,
            p_memory_barriers: &vk::MemoryBarrier2KHR {
                src_stage_mask: vk::PipelineStageFlags2KHR::ACCELERATION_STRUCTURE_BUILD_KHR,
                dst_stage_mask: vk::PipelineStageFlags2KHR::ACCELERATION_STRUCTURE_BUILD_KHR,
                src_access_mask: vk::AccessFlags2KHR::MEMORY_READ | vk::AccessFlags2::MEMORY_WRITE,
                dst_access_mask: vk::AccessFlags2KHR::MEMORY_READ | vk::AccessFlags2::MEMORY_WRITE,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    accel_struct_loader.cmd_build_acceleration_structures(
        command_buffer,
        &[vk::AccelerationStructureBuildGeometryInfoKHR {
            ty: vk::AccelerationStructureTypeKHR::TOP_LEVEL,
            flags: vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE,
            mode: vk::BuildAccelerationStructureModeKHR::BUILD,
            dst_acceleration_structure: tlas,
            geometry_count: 1,
            p_geometries: &vk::AccelerationStructureGeometryKHR {
                geometry_type: vk::GeometryTypeKHR::INSTANCES,
                geometry: vk::AccelerationStructureGeometryDataKHR {
                    instances: vk::AccelerationStructureGeometryInstancesDataKHR {
                        array_of_pointers: vk::FALSE,
                        data: vk::DeviceOrHostAddressConstKHR {
                            device_address: device.get_buffer_device_address(
                                &vk::BufferDeviceAddressInfo {
                                    buffer: tlas_input_buf,
                                    ..Default::default()
                                },
                            ),
                        },
                        ..Default::default()
                    },
                },
                flags: vk::GeometryFlagsKHR::OPAQUE,
                ..Default::default()
            },
            scratch_data: vk::DeviceOrHostAddressKHR {
                device_address: device.get_buffer_device_address(&vk::BufferDeviceAddressInfo {
                    buffer: scratch_buf,
                    ..Default::default()
                }),
            },
            ..Default::default()
        }],
        &[&[vk::AccelerationStructureBuildRangeInfoKHR {
            primitive_count: 1,
            primitive_offset: 0,
            first_vertex: 0,
            transform_offset: 0,
        }]],
    );
    device.end_command_buffer(command_buffer).unwrap();

    let queue = device.get_device_queue(0, 0);
    device
        .queue_submit(
            queue,
            &[vk::SubmitInfo {
                command_buffer_count: 1,
                p_command_buffers: &command_buffer,
                ..Default::default()
            }],
            vk::Fence::null(),
        )
        .unwrap();
    device.queue_wait_idle(queue).unwrap();
    device
        .reset_command_pool(command_pool, Default::default())
        .unwrap();

    let desc_set_layout = device
        .create_descriptor_set_layout(
            &vk::DescriptorSetLayoutCreateInfo {
                flags: vk::DescriptorSetLayoutCreateFlags::empty(),
                binding_count: 2,
                p_bindings: [
                    vk::DescriptorSetLayoutBinding {
                        binding: 0,
                        descriptor_type: vk::DescriptorType::ACCELERATION_STRUCTURE_KHR,
                        descriptor_count: 1,
                        stage_flags: vk::ShaderStageFlags::RAYGEN_KHR,
                        ..Default::default()
                    },
                    vk::DescriptorSetLayoutBinding {
                        binding: 1,
                        descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
                        descriptor_count: 1,
                        stage_flags: vk::ShaderStageFlags::RAYGEN_KHR | vk::ShaderStageFlags::INTERSECTION_KHR
                            | vk::ShaderStageFlags::MISS_KHR | vk::ShaderStageFlags::CLOSEST_HIT_KHR,
                        ..Default::default()
                    },
                ]
                .as_slice()
                .as_ptr(),
                ..Default::default()
            },
            None,
        )
        .unwrap();
    let pipeline_layout = device
        .create_pipeline_layout(
            &vk::PipelineLayoutCreateInfo {
                set_layout_count: 1,
                p_set_layouts: &desc_set_layout,
                ..Default::default()
            },
            None,
        )
        .unwrap();
    // Now, create pipeline.

    let raygen_code = include_bytes!("test.rgen.spv");
    let miss_code = include_bytes!("test.rmiss.spv");
    let rint_code = include_bytes!("test.rint.spv");
    let rchit_code = include_bytes!("test.rchit.spv");
    let rtx_pipeline_loader = ash::extensions::khr::RayTracingPipeline::new(&instance, &device);
    let pipeline = rtx_pipeline_loader
        .create_ray_tracing_pipelines(
            vk::DeferredOperationKHR::null(),
            vk::PipelineCache::null(),
            &[vk::RayTracingPipelineCreateInfoKHR {
                stage_count: 4,
                p_stages: [
                    vk::PipelineShaderStageCreateInfo {
                        stage: vk::ShaderStageFlags::RAYGEN_KHR,
                        module: device
                            .create_shader_module(
                                &vk::ShaderModuleCreateInfo {
                                    flags: vk::ShaderModuleCreateFlags::empty(),
                                    code_size: raygen_code.len(),
                                    p_code: raygen_code.as_ptr() as *const _,
                                    ..Default::default()
                                },
                                None,
                            )
                            .unwrap(),
                        p_name: b"main\0".as_ptr() as *const i8,
                        ..Default::default()
                    },
                    vk::PipelineShaderStageCreateInfo {
                        stage: vk::ShaderStageFlags::MISS_KHR,
                        module: device
                            .create_shader_module(
                                &vk::ShaderModuleCreateInfo {
                                    flags: vk::ShaderModuleCreateFlags::empty(),
                                    code_size: miss_code.len(),
                                    p_code: miss_code.as_ptr() as *const _,
                                    ..Default::default()
                                },
                                None,
                            )
                            .unwrap(),
                        p_name: b"main\0".as_ptr() as *const i8,
                        ..Default::default()
                    },
                    vk::PipelineShaderStageCreateInfo {
                        stage: vk::ShaderStageFlags::INTERSECTION_KHR,
                        module: device
                            .create_shader_module(
                                &vk::ShaderModuleCreateInfo {
                                    flags: vk::ShaderModuleCreateFlags::empty(),
                                    code_size: rint_code.len(),
                                    p_code: rint_code.as_ptr() as *const _,
                                    ..Default::default()
                                },
                                None,
                            )
                            .unwrap(),
                        p_name: b"main\0".as_ptr() as *const i8,
                        ..Default::default()
                    },
                    vk::PipelineShaderStageCreateInfo {
                        stage: vk::ShaderStageFlags::CLOSEST_HIT_KHR,
                        module: device
                            .create_shader_module(
                                &vk::ShaderModuleCreateInfo {
                                    flags: vk::ShaderModuleCreateFlags::empty(),
                                    code_size: rchit_code.len(),
                                    p_code: rchit_code.as_ptr() as *const _,
                                    ..Default::default()
                                },
                                None,
                            )
                            .unwrap(),
                        p_name: b"main\0".as_ptr() as *const i8,
                        ..Default::default()
                    },
                ]
                .as_slice()
                .as_ptr(),
                group_count: 3,
                p_groups: [
                    vk::RayTracingShaderGroupCreateInfoKHR {
                        ty: vk::RayTracingShaderGroupTypeKHR::GENERAL,
                        general_shader: 0, // rgen
                        any_hit_shader: vk::SHADER_UNUSED_KHR,
                        closest_hit_shader: vk::SHADER_UNUSED_KHR,
                        intersection_shader: vk::SHADER_UNUSED_KHR,
                        ..Default::default()
                    },
                    vk::RayTracingShaderGroupCreateInfoKHR {
                        ty: vk::RayTracingShaderGroupTypeKHR::GENERAL,
                        general_shader: 1, // rmiss
                        any_hit_shader: vk::SHADER_UNUSED_KHR,
                        closest_hit_shader: vk::SHADER_UNUSED_KHR,
                        intersection_shader: vk::SHADER_UNUSED_KHR,
                        ..Default::default()
                    },
                    vk::RayTracingShaderGroupCreateInfoKHR {
                        ty: vk::RayTracingShaderGroupTypeKHR::PROCEDURAL_HIT_GROUP,
                        intersection_shader: 2,
                        any_hit_shader: vk::SHADER_UNUSED_KHR,
                        closest_hit_shader: 3,
                        general_shader: vk::SHADER_UNUSED_KHR,
                        ..Default::default()
                    },
                ]
                .as_slice()
                .as_ptr(),
                max_pipeline_ray_recursion_depth: 1,
                layout: pipeline_layout,
                ..Default::default()
            }],
            None,
        )
        .unwrap()[0];

    let mut properties = vk::PhysicalDeviceProperties2::default();
    let mut rtx_pipeline_properties: vk::PhysicalDeviceRayTracingPipelinePropertiesKHR =
        vk::PhysicalDeviceRayTracingPipelinePropertiesKHR::default();
    properties.p_next = &mut rtx_pipeline_properties as *mut _ as *mut _;
    instance.get_physical_device_properties2(pdevice, &mut properties);
    println!("{:#?}", rtx_pipeline_properties);

    let group_handles = rtx_pipeline_loader
        .get_ray_tracing_shader_group_handles(
            pipeline,
            0,
            3,
            rtx_pipeline_properties.shader_group_handle_size as usize * 3, // On both NV and Intel, this is 32 * 3
        )
        .unwrap();

    let (results_buffer, results_memory) = create_buffer(
        &device,
        memory_type_index,
        1000,
        vk::BufferUsageFlags::STORAGE_BUFFER,
    );
    {
        let ptr = device
            .map_memory(results_memory, 0, 1000, Default::default())
            .unwrap() as *mut u8;
        for i in 0..1000 {
            *ptr.add(i) = 0;
        }
        device.unmap_memory(results_memory);
    }

    let (sbt1_buffer, sbt1_memory) = create_buffer(
        &device,
        memory_type_index,
        1000,
        vk::BufferUsageFlags::SHADER_BINDING_TABLE_KHR
            | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
    );
    {
        let ptr = device
            .map_memory(sbt1_memory, 0, 1000, Default::default())
            .unwrap() as *mut u8;

        let sbt_data: [u32; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
        std::ptr::copy_nonoverlapping(group_handles.as_ptr(), ptr.add(0 + BASE_OFFSET), 32);
        std::ptr::copy_nonoverlapping(sbt_data.as_ptr() as *const u8, ptr.add(0 + BASE_OFFSET + 32), 64);

        std::ptr::copy_nonoverlapping(group_handles.as_ptr().add(32), ptr.add(96 + BASE_OFFSET), 32);

        std::ptr::copy_nonoverlapping(group_handles.as_ptr().add(64), ptr.add(96*2 + BASE_OFFSET), 32);
        device.unmap_memory(sbt1_memory);
    }

    let descriptor_pool = device
        .create_descriptor_pool(
            &vk::DescriptorPoolCreateInfo {
                max_sets: 1,
                pool_size_count: 2,
                p_pool_sizes: [
                    vk::DescriptorPoolSize {
                        ty: vk::DescriptorType::STORAGE_BUFFER,
                        descriptor_count: 1,
                    },
                    vk::DescriptorPoolSize {
                        ty: vk::DescriptorType::ACCELERATION_STRUCTURE_KHR,
                        descriptor_count: 1,
                    },
                ]
                .as_slice()
                .as_ptr(),
                ..Default::default()
            },
            None,
        )
        .unwrap();
    let desc_set = device
        .allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo {
            descriptor_pool,
            descriptor_set_count: 1,
            p_set_layouts: &desc_set_layout,
            ..Default::default()
        })
        .unwrap()[0];

    device.update_descriptor_sets(
        &[
            vk::WriteDescriptorSet {
                dst_set: desc_set,
                dst_binding: 1,
                descriptor_count: 1,
                descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
                p_buffer_info: &vk::DescriptorBufferInfo {
                    buffer: results_buffer,
                    offset: 0,
                    range: 1000,
                },
                ..Default::default()
            },
            vk::WriteDescriptorSet {
                dst_set: desc_set,
                dst_binding: 0,
                descriptor_count: 1,
                descriptor_type: vk::DescriptorType::ACCELERATION_STRUCTURE_KHR,
                p_next: &vk::WriteDescriptorSetAccelerationStructureKHR {
                    acceleration_structure_count: 1,
                    p_acceleration_structures: &tlas,
                    ..Default::default()
                } as *const _ as *const c_void,
                ..Default::default()
            },
        ],
        &[],
    );




    device.begin_command_buffer(command_buffer, &Default::default()).unwrap();
    device.cmd_bind_pipeline(command_buffer, vk::PipelineBindPoint::RAY_TRACING_KHR, pipeline);
    device.cmd_bind_descriptor_sets(command_buffer, vk::PipelineBindPoint::RAY_TRACING_KHR, pipeline_layout, 0, &[desc_set], &[]);
    let base_address = device.get_buffer_device_address(
        &vk::BufferDeviceAddressInfo {
            buffer: sbt1_buffer,
            ..Default::default()
        }
    );
    assert!(base_address % 64 == 0);
    rtx_pipeline_loader.cmd_trace_rays(command_buffer,
        &vk::StridedDeviceAddressRegionKHR {
            device_address: base_address + 0 + BASE_OFFSET as u64,
            stride: 96,
            size: 96,
        },
        &vk::StridedDeviceAddressRegionKHR {
            device_address: base_address + 96 + BASE_OFFSET as u64,
            stride: 96,
            size: 96,
        },
        &vk::StridedDeviceAddressRegionKHR {
            device_address: base_address + 96*2 + BASE_OFFSET as u64,
            stride: 96,
            size: 96,
        }, &vk::StridedDeviceAddressRegionKHR::default(), 1, 1, 1);
    device.end_command_buffer(command_buffer).unwrap();

    device.queue_submit(queue, &[vk::SubmitInfo {
        command_buffer_count: 1,
        p_command_buffers: &command_buffer,
        ..Default::default()
    }], vk::Fence::null()).unwrap();
    device.queue_wait_idle(queue).unwrap();



    let ptr = device.map_memory(results_memory, 0, 1000, Default::default()).unwrap() as *mut u8;
    let results = std::slice::from_raw_parts(ptr as *const u8 as *const u32, 1000);

    for i in 0..16 {
        // The ray gen shader copies all raygen data into the results buffer. Assert that they're the same as the one
        // we put in, on line 633.
        assert_eq!(results[i], i as u32);
    }
    assert_eq!(results[17], 12777); // intersection shader ran
    assert_eq!(results[16], 120000); // closest hit shader ran
    println!("Test passed!");
}
