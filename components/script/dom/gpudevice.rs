/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

#![allow(unsafe_code)]

use crate::dom::bindings::cell::DomRefCell;
use crate::dom::bindings::codegen::Bindings::GPUBindGroupBinding::{
    GPUBindGroupDescriptor, GPUBindingResource,
};
use crate::dom::bindings::codegen::Bindings::GPUBindGroupLayoutBinding::{
    GPUBindGroupLayoutDescriptor, GPUBindingType,
};
use crate::dom::bindings::codegen::Bindings::GPUBufferBinding::GPUBufferDescriptor;
use crate::dom::bindings::codegen::Bindings::GPUComputePipelineBinding::GPUComputePipelineDescriptor;
use crate::dom::bindings::codegen::Bindings::GPUDeviceBinding::{
    GPUCommandEncoderDescriptor, GPUDeviceMethods,
};
use crate::dom::bindings::codegen::Bindings::GPUPipelineLayoutBinding::GPUPipelineLayoutDescriptor;
use crate::dom::bindings::codegen::Bindings::GPURenderPipelineBinding::{
    GPUBlendDescriptor, GPUBlendFactor, GPUBlendOperation, GPUCullMode, GPUFrontFace,
    GPUIndexFormat, GPUInputStepMode, GPUPrimitiveTopology, GPURenderPipelineDescriptor,
    GPUStencilOperation, GPUVertexFormat,
};
use crate::dom::bindings::codegen::Bindings::GPUSamplerBinding::{
    GPUAddressMode, GPUCompareFunction, GPUFilterMode, GPUSamplerDescriptor,
};
use crate::dom::bindings::codegen::Bindings::GPUShaderModuleBinding::GPUShaderModuleDescriptor;
use crate::dom::bindings::codegen::Bindings::GPUTextureBinding::{
    GPUExtent3D, GPUExtent3DDict, GPUTextureComponentType, GPUTextureDescriptor,
    GPUTextureDimension, GPUTextureFormat,
};
use crate::dom::bindings::codegen::Bindings::GPUTextureViewBinding::GPUTextureViewDimension;
use crate::dom::bindings::codegen::Bindings::GPUValidationErrorBinding::{
    GPUError, GPUErrorFilter,
};
use crate::dom::bindings::codegen::UnionTypes::Uint32ArrayOrString;
use crate::dom::bindings::error::Error;
use crate::dom::bindings::reflector::{reflect_dom_object, DomObject};
use crate::dom::bindings::root::{Dom, DomRoot};
use crate::dom::bindings::str::USVString;
use crate::dom::bindings::trace::RootedTraceableBox;
use crate::dom::eventtarget::EventTarget;
use crate::dom::globalscope::GlobalScope;
use crate::dom::gpuadapter::GPUAdapter;
use crate::dom::gpubindgroup::GPUBindGroup;
use crate::dom::gpubindgrouplayout::GPUBindGroupLayout;
use crate::dom::gpubuffer::{GPUBuffer, GPUBufferMapInfo, GPUBufferState};
use crate::dom::gpucommandencoder::GPUCommandEncoder;
use crate::dom::gpucomputepipeline::GPUComputePipeline;
use crate::dom::gpupipelinelayout::GPUPipelineLayout;
use crate::dom::gpuqueue::GPUQueue;
use crate::dom::gpurenderpipeline::GPURenderPipeline;
use crate::dom::gpusampler::GPUSampler;
use crate::dom::gpushadermodule::GPUShaderModule;
use crate::dom::gputexture::GPUTexture;
use crate::dom::promise::Promise;
use crate::realms::InRealm;
use crate::script_runtime::JSContext as SafeJSContext;
use arrayvec::ArrayVec;
use dom_struct::dom_struct;
use js::jsapi::{Heap, JSObject};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::NonNull;
use std::rc::Rc;
use std::string::String;
use webgpu::wgpu::binding_model::BufferBinding;
use webgpu::{self, wgt, WebGPU, WebGPUBindings, WebGPURequest};

type ErrorScopeId = u64;

#[derive(JSTraceable, MallocSizeOf)]
struct ErrorScopeInfo {
    filter: GPUErrorFilter,
    op_count: u64,
    #[ignore_malloc_size_of = "defined in webgpu"]
    error: Option<GPUError>,
    #[ignore_malloc_size_of = "promises are hard"]
    promise: Option<Rc<Promise>>,
}

#[derive(JSTraceable, MallocSizeOf)]
struct ScopeContext {
    error_scopes: HashMap<ErrorScopeId, ErrorScopeInfo>,
    scope_stack: Vec<ErrorScopeId>,
    next_scope_id: ErrorScopeId,
}

#[dom_struct]
pub struct GPUDevice {
    eventtarget: EventTarget,
    #[ignore_malloc_size_of = "channels are hard"]
    channel: WebGPU,
    adapter: Dom<GPUAdapter>,
    #[ignore_malloc_size_of = "mozjs"]
    extensions: Heap<*mut JSObject>,
    #[ignore_malloc_size_of = "mozjs"]
    limits: Heap<*mut JSObject>,
    label: DomRefCell<Option<USVString>>,
    device: webgpu::WebGPUDevice,
    default_queue: Dom<GPUQueue>,
    scope_context: DomRefCell<ScopeContext>,
    #[ignore_malloc_size_of = "promises are hard"]
    lost_promise: DomRefCell<Option<Rc<Promise>>>,
    #[ignore_malloc_size_of = "defined in webgpu"]
    bind_group_layouts:
        DomRefCell<HashMap<Vec<wgt::BindGroupLayoutEntry>, Dom<GPUBindGroupLayout>>>,
}

impl GPUDevice {
    fn new_inherited(
        channel: WebGPU,
        adapter: &GPUAdapter,
        extensions: Heap<*mut JSObject>,
        limits: Heap<*mut JSObject>,
        device: webgpu::WebGPUDevice,
        queue: &GPUQueue,
    ) -> Self {
        Self {
            eventtarget: EventTarget::new_inherited(),
            channel,
            adapter: Dom::from_ref(adapter),
            extensions,
            limits,
            label: DomRefCell::new(None),
            device,
            default_queue: Dom::from_ref(queue),
            scope_context: DomRefCell::new(ScopeContext {
                error_scopes: HashMap::new(),
                scope_stack: Vec::new(),
                next_scope_id: 0,
            }),
            lost_promise: DomRefCell::new(None),
            bind_group_layouts: DomRefCell::new(HashMap::new()),
        }
    }

    pub fn new(
        global: &GlobalScope,
        channel: WebGPU,
        adapter: &GPUAdapter,
        extensions: Heap<*mut JSObject>,
        limits: Heap<*mut JSObject>,
        device: webgpu::WebGPUDevice,
        queue: webgpu::WebGPUQueue,
    ) -> DomRoot<Self> {
        let queue = GPUQueue::new(global, channel.clone(), queue);
        reflect_dom_object(
            Box::new(GPUDevice::new_inherited(
                channel, adapter, extensions, limits, device, &queue,
            )),
            global,
        )
    }
}

impl GPUDevice {
    pub fn id(&self) -> webgpu::WebGPUDevice {
        self.device
    }

    pub fn handle_server_msg(&self, scope: ErrorScopeId, result: Result<(), GPUError>) {
        let mut context = self.scope_context.borrow_mut();
        let remove = if let Some(mut err_scope) = context.error_scopes.get_mut(&scope) {
            err_scope.op_count -= 1;
            match result {
                Ok(()) => {},
                Err(e) => {
                    if err_scope.error.is_none() {
                        err_scope.error = Some(e);
                    }
                },
            }
            if let Some(ref promise) = err_scope.promise {
                if !promise.is_fulfilled() {
                    if let Some(ref e) = err_scope.error {
                        match e {
                            GPUError::GPUValidationError(v) => promise.resolve_native(&v),
                            GPUError::GPUOutOfMemoryError(w) => promise.resolve_native(&w),
                        }
                    } else if err_scope.op_count == 0 {
                        promise.resolve_native(&None::<GPUError>);
                    }
                }
            }
            err_scope.op_count == 0 && err_scope.promise.is_some()
        } else {
            warn!("Could not find ErrroScope with Id({})", scope);
            false
        };
        if remove {
            let _ = context.error_scopes.remove(&scope);
        }
    }

    fn use_current_scope(&self) -> Option<ErrorScopeId> {
        let mut context = self.scope_context.borrow_mut();
        let scope_id = context.scope_stack.last().cloned();
        scope_id.and_then(|s_id| {
            context.error_scopes.get_mut(&s_id).map(|mut scope| {
                scope.op_count += 1;
                s_id
            })
        })
    }
}

impl GPUDeviceMethods for GPUDevice {
    /// https://gpuweb.github.io/gpuweb/#dom-gpudevice-adapter
    fn Adapter(&self) -> DomRoot<GPUAdapter> {
        DomRoot::from_ref(&self.adapter)
    }

    /// https://gpuweb.github.io/gpuweb/#dom-gpudevice-extensions
    fn Extensions(&self, _cx: SafeJSContext) -> NonNull<JSObject> {
        NonNull::new(self.extensions.get()).unwrap()
    }

    /// https://gpuweb.github.io/gpuweb/#dom-gpudevice-limits
    fn Limits(&self, _cx: SafeJSContext) -> NonNull<JSObject> {
        NonNull::new(self.extensions.get()).unwrap()
    }

    /// https://gpuweb.github.io/gpuweb/#dom-gpudevice-defaultqueue
    fn DefaultQueue(&self) -> DomRoot<GPUQueue> {
        DomRoot::from_ref(&self.default_queue)
    }

    /// https://gpuweb.github.io/gpuweb/#dom-gpuobjectbase-label
    fn GetLabel(&self) -> Option<USVString> {
        self.label.borrow().clone()
    }

    /// https://gpuweb.github.io/gpuweb/#dom-gpuobjectbase-label
    fn SetLabel(&self, value: Option<USVString>) {
        *self.label.borrow_mut() = value;
    }

    /// https://gpuweb.github.io/gpuweb/#dom-gpudevice-lost
    fn Lost(&self, comp: InRealm) -> Rc<Promise> {
        let promise = Promise::new_in_current_realm(&self.global(), comp);
        *self.lost_promise.borrow_mut() = Some(promise.clone());
        promise
    }

    /// https://gpuweb.github.io/gpuweb/#dom-gpudevice-createbuffer
    fn CreateBuffer(&self, descriptor: &GPUBufferDescriptor) -> DomRoot<GPUBuffer> {
        let wgpu_descriptor = wgt::BufferDescriptor {
            label: descriptor
                .parent
                .label
                .as_ref()
                .map(|s| String::from(s.as_ref())),
            size: descriptor.size,
            usage: match wgt::BufferUsage::from_bits(descriptor.usage) {
                Some(u) => u,
                None => wgt::BufferUsage::empty(),
            },
            mapped_at_creation: descriptor.mappedAtCreation,
        };
        let id = self
            .global()
            .wgpu_id_hub()
            .lock()
            .create_buffer_id(self.device.0.backend());
        self.channel
            .0
            .send(WebGPURequest::CreateBuffer {
                device_id: self.device.0,
                buffer_id: id,
                descriptor: wgpu_descriptor,
            })
            .expect("Failed to create WebGPU buffer");

        let buffer = webgpu::WebGPUBuffer(id);
        let map_info;
        let state;
        if descriptor.mappedAtCreation {
            let buf_data = vec![0u8; descriptor.size as usize];
            map_info = DomRefCell::new(Some(GPUBufferMapInfo {
                mapping: Rc::new(RefCell::new(buf_data)),
                mapping_range: 0..descriptor.size,
                mapped_ranges: Vec::new(),
                js_buffers: Vec::new(),
                map_mode: None,
            }));
            state = GPUBufferState::MappedAtCreation;
        } else {
            map_info = DomRefCell::new(None);
            state = GPUBufferState::Unmapped;
        }

        GPUBuffer::new(
            &self.global(),
            self.channel.clone(),
            buffer,
            self.device,
            state,
            descriptor.size,
            map_info,
        )
    }

    /// https://gpuweb.github.io/gpuweb/#GPUDevice-createBindGroupLayout
    #[allow(non_snake_case)]
    fn CreateBindGroupLayout(
        &self,
        descriptor: &GPUBindGroupLayoutDescriptor,
    ) -> DomRoot<GPUBindGroupLayout> {
        let entries = descriptor
            .entries
            .iter()
            .map(|bind| {
                let visibility = match wgt::ShaderStage::from_bits(bind.visibility) {
                    Some(visibility) => visibility,
                    None => wgt::ShaderStage::from_bits(0).unwrap(),
                };
                let ty = match bind.type_ {
                    GPUBindingType::Uniform_buffer => wgt::BindingType::UniformBuffer {
                        dynamic: bind.hasDynamicOffset,
                        min_binding_size: wgt::BufferSize::new(bind.minBufferBindingSize),
                    },
                    GPUBindingType::Storage_buffer => wgt::BindingType::StorageBuffer {
                        dynamic: bind.hasDynamicOffset,
                        min_binding_size: wgt::BufferSize::new(bind.minBufferBindingSize),
                        readonly: false,
                    },
                    GPUBindingType::Readonly_storage_buffer => wgt::BindingType::StorageBuffer {
                        dynamic: bind.hasDynamicOffset,
                        min_binding_size: wgt::BufferSize::new(bind.minBufferBindingSize),
                        readonly: true,
                    },
                    GPUBindingType::Sampled_texture => wgt::BindingType::SampledTexture {
                        dimension: bind
                            .viewDimension
                            .map_or(wgt::TextureViewDimension::D2, |v| {
                                convert_texture_view_dimension(v)
                            }),
                        component_type: if let Some(c) = bind.textureComponentType {
                            match c {
                                GPUTextureComponentType::Float => wgt::TextureComponentType::Float,
                                GPUTextureComponentType::Sint => wgt::TextureComponentType::Sint,
                                GPUTextureComponentType::Uint => wgt::TextureComponentType::Uint,
                            }
                        } else {
                            wgt::TextureComponentType::Float
                        },
                        multisampled: bind.multisampled,
                    },
                    GPUBindingType::Readonly_storage_texture => wgt::BindingType::StorageTexture {
                        dimension: bind
                            .viewDimension
                            .map_or(wgt::TextureViewDimension::D2, |v| {
                                convert_texture_view_dimension(v)
                            }),
                        format: bind
                            .storageTextureFormat
                            .map_or(wgt::TextureFormat::Bgra8UnormSrgb, |f| {
                                convert_texture_format(f)
                            }),
                        readonly: true,
                    },
                    GPUBindingType::Writeonly_storage_texture => wgt::BindingType::StorageTexture {
                        dimension: bind
                            .viewDimension
                            .map_or(wgt::TextureViewDimension::D2, |v| {
                                convert_texture_view_dimension(v)
                            }),
                        format: bind
                            .storageTextureFormat
                            .map_or(wgt::TextureFormat::Bgra8UnormSrgb, |f| {
                                convert_texture_format(f)
                            }),
                        readonly: true,
                    },
                    GPUBindingType::Sampler => wgt::BindingType::Sampler { comparison: false },
                    GPUBindingType::Comparison_sampler => {
                        wgt::BindingType::Sampler { comparison: true }
                    },
                };

                wgt::BindGroupLayoutEntry::new(bind.binding, visibility, ty)
            })
            .collect::<Vec<_>>();

        let scope_id = self.use_current_scope();

        // Check for equivalent GPUBindGroupLayout
        {
            let layout = self
                .bind_group_layouts
                .borrow()
                .get(&entries)
                .map(|bgl| DomRoot::from_ref(&**bgl));
            if let Some(l) = layout {
                if let Some(i) = scope_id {
                    self.handle_server_msg(i, Ok(()));
                }
                return l;
            }
        }

        let bind_group_layout_id = self
            .global()
            .wgpu_id_hub()
            .lock()
            .create_bind_group_layout_id(self.device.0.backend());
        self.channel
            .0
            .send(WebGPURequest::CreateBindGroupLayout {
                device_id: self.device.0,
                bind_group_layout_id,
                entries: entries.clone(),
                scope_id,
                label: descriptor
                    .parent
                    .label
                    .as_ref()
                    .map(|s| String::from(s.as_ref())),
            })
            .expect("Failed to create WebGPU BindGroupLayout");

        let bgl = webgpu::WebGPUBindGroupLayout(bind_group_layout_id);

        let layout = GPUBindGroupLayout::new(&self.global(), bgl);

        self.bind_group_layouts
            .borrow_mut()
            .insert(entries, Dom::from_ref(&*layout));

        layout
    }

    /// https://gpuweb.github.io/gpuweb/#dom-gpudevice-createpipelinelayout
    fn CreatePipelineLayout(
        &self,
        descriptor: &GPUPipelineLayoutDescriptor,
    ) -> DomRoot<GPUPipelineLayout> {
        let mut bgl_ids = Vec::new();
        descriptor
            .bindGroupLayouts
            .iter()
            .for_each(|each| bgl_ids.push(each.id().0));

        let scope_id = self.use_current_scope();

        let pipeline_layout_id = self
            .global()
            .wgpu_id_hub()
            .lock()
            .create_pipeline_layout_id(self.device.0.backend());
        self.channel
            .0
            .send(WebGPURequest::CreatePipelineLayout {
                device_id: self.device.0,
                pipeline_layout_id,
                bind_group_layouts: bgl_ids,
                scope_id,
            })
            .expect("Failed to create WebGPU PipelineLayout");

        let pipeline_layout = webgpu::WebGPUPipelineLayout(pipeline_layout_id);
        GPUPipelineLayout::new(&self.global(), pipeline_layout)
    }

    /// https://gpuweb.github.io/gpuweb/#dom-gpudevice-createbindgroup
    fn CreateBindGroup(&self, descriptor: &GPUBindGroupDescriptor) -> DomRoot<GPUBindGroup> {
        let entries = descriptor
            .entries
            .iter()
            .map(|bind| {
                (
                    bind.binding,
                    match bind.resource {
                        GPUBindingResource::GPUSampler(ref s) => WebGPUBindings::Sampler(s.id().0),
                        GPUBindingResource::GPUTextureView(ref t) => {
                            WebGPUBindings::TextureView(t.id().0)
                        },
                        GPUBindingResource::GPUBufferBindings(ref b) => {
                            WebGPUBindings::Buffer(BufferBinding {
                                buffer_id: b.buffer.id().0,
                                offset: b.offset,
                                size: b.size.and_then(wgt::BufferSize::new),
                            })
                        },
                    },
                )
            })
            .collect::<Vec<_>>();

        let scope_id = self.use_current_scope();

        let bind_group_id = self
            .global()
            .wgpu_id_hub()
            .lock()
            .create_bind_group_id(self.device.0.backend());
        self.channel
            .0
            .send(WebGPURequest::CreateBindGroup {
                device_id: self.device.0,
                bind_group_id,
                bind_group_layout_id: descriptor.layout.id().0,
                entries,
                scope_id,
                label: descriptor
                    .parent
                    .label
                    .as_ref()
                    .map(|s| String::from(s.as_ref())),
            })
            .expect("Failed to create WebGPU BindGroup");

        let bind_group = webgpu::WebGPUBindGroup(bind_group_id);

        GPUBindGroup::new(&self.global(), bind_group, self.device, &*descriptor.layout)
    }

    /// https://gpuweb.github.io/gpuweb/#dom-gpudevice-createshadermodule
    fn CreateShaderModule(
        &self,
        descriptor: RootedTraceableBox<GPUShaderModuleDescriptor>,
    ) -> DomRoot<GPUShaderModule> {
        let program: Vec<u32> = match &descriptor.code {
            Uint32ArrayOrString::Uint32Array(program) => program.to_vec(),
            Uint32ArrayOrString::String(program) => {
                program.chars().map(|c| c as u32).collect::<Vec<u32>>()
            },
        };
        let program_id = self
            .global()
            .wgpu_id_hub()
            .lock()
            .create_shader_module_id(self.device.0.backend());
        self.channel
            .0
            .send(WebGPURequest::CreateShaderModule {
                device_id: self.device.0,
                program_id,
                program,
            })
            .expect("Failed to create WebGPU ShaderModule");

        let shader_module = webgpu::WebGPUShaderModule(program_id);
        GPUShaderModule::new(&self.global(), shader_module)
    }

    /// https://gpuweb.github.io/gpuweb/#dom-gpudevice-createcomputepipeline
    fn CreateComputePipeline(
        &self,
        descriptor: &GPUComputePipelineDescriptor,
    ) -> DomRoot<GPUComputePipeline> {
        let pipeline = descriptor.parent.layout.id();
        let program = descriptor.computeStage.module.id();
        let entry_point = descriptor.computeStage.entryPoint.to_string();
        let compute_pipeline_id = self
            .global()
            .wgpu_id_hub()
            .lock()
            .create_compute_pipeline_id(self.device.0.backend());

        let scope_id = self.use_current_scope();

        self.channel
            .0
            .send(WebGPURequest::CreateComputePipeline {
                device_id: self.device.0,
                scope_id,
                compute_pipeline_id,
                pipeline_layout_id: pipeline.0,
                program_id: program.0,
                entry_point,
            })
            .expect("Failed to create WebGPU ComputePipeline");

        let compute_pipeline = webgpu::WebGPUComputePipeline(compute_pipeline_id);
        GPUComputePipeline::new(&self.global(), compute_pipeline)
    }

    /// https://gpuweb.github.io/gpuweb/#dom-gpudevice-createcommandencoder
    fn CreateCommandEncoder(
        &self,
        descriptor: &GPUCommandEncoderDescriptor,
    ) -> DomRoot<GPUCommandEncoder> {
        let command_encoder_id = self
            .global()
            .wgpu_id_hub()
            .lock()
            .create_command_encoder_id(self.device.0.backend());
        self.channel
            .0
            .send(WebGPURequest::CreateCommandEncoder {
                device_id: self.device.0,
                command_encoder_id,
                label: descriptor
                    .parent
                    .label
                    .as_ref()
                    .map(|s| String::from(s.as_ref())),
            })
            .expect("Failed to create WebGPU command encoder");

        let encoder = webgpu::WebGPUCommandEncoder(command_encoder_id);

        GPUCommandEncoder::new(
            &self.global(),
            self.channel.clone(),
            self.device,
            encoder,
            true,
        )
    }

    /// https://gpuweb.github.io/gpuweb/#dom-gpudevice-createtexture
    fn CreateTexture(&self, descriptor: &GPUTextureDescriptor) -> DomRoot<GPUTexture> {
        let size = convert_texture_size_to_dict(&descriptor.size);
        let desc = wgt::TextureDescriptor {
            label: descriptor
                .parent
                .label
                .as_ref()
                .map(|s| String::from(s.as_ref())),
            size: convert_texture_size_to_wgt(&size),
            mip_level_count: descriptor.mipLevelCount,
            sample_count: descriptor.sampleCount,
            dimension: match descriptor.dimension {
                GPUTextureDimension::_1d => wgt::TextureDimension::D1,
                GPUTextureDimension::_2d => wgt::TextureDimension::D2,
                GPUTextureDimension::_3d => wgt::TextureDimension::D3,
            },
            format: convert_texture_format(descriptor.format),
            usage: match wgt::TextureUsage::from_bits(descriptor.usage) {
                Some(t) => t,
                None => wgt::TextureUsage::empty(),
            },
        };

        let texture_id = self
            .global()
            .wgpu_id_hub()
            .lock()
            .create_texture_id(self.device.0.backend());

        self.channel
            .0
            .send(WebGPURequest::CreateTexture {
                device_id: self.device.0,
                texture_id,
                descriptor: desc,
            })
            .expect("Failed to create WebGPU Texture");

        let texture = webgpu::WebGPUTexture(texture_id);

        GPUTexture::new(
            &self.global(),
            texture,
            self.device,
            self.channel.clone(),
            size,
            descriptor.mipLevelCount,
            descriptor.sampleCount,
            descriptor.dimension,
            descriptor.format,
            descriptor.usage,
        )
    }

    /// https://gpuweb.github.io/gpuweb/#dom-gpudevice-createsampler
    fn CreateSampler(&self, descriptor: &GPUSamplerDescriptor) -> DomRoot<GPUSampler> {
        let sampler_id = self
            .global()
            .wgpu_id_hub()
            .lock()
            .create_sampler_id(self.device.0.backend());
        let compare_enable = descriptor.compare.is_some();
        let desc = wgt::SamplerDescriptor {
            label: descriptor
                .parent
                .label
                .as_ref()
                .map(|s| String::from(s.as_ref())),
            address_mode_u: convert_address_mode(descriptor.addressModeU),
            address_mode_v: convert_address_mode(descriptor.addressModeV),
            address_mode_w: convert_address_mode(descriptor.addressModeW),
            mag_filter: convert_filter_mode(descriptor.magFilter),
            min_filter: convert_filter_mode(descriptor.minFilter),
            mipmap_filter: convert_filter_mode(descriptor.mipmapFilter),
            lod_min_clamp: *descriptor.lodMinClamp,
            lod_max_clamp: *descriptor.lodMaxClamp,
            compare: descriptor.compare.map(|c| convert_compare_function(c)),
            anisotropy_clamp: None,
            ..Default::default()
        };
        self.channel
            .0
            .send(WebGPURequest::CreateSampler {
                device_id: self.device.0,
                sampler_id,
                descriptor: desc,
            })
            .expect("Failed to create WebGPU sampler");

        let sampler = webgpu::WebGPUSampler(sampler_id);

        GPUSampler::new(&self.global(), self.device, compare_enable, sampler)
    }

    /// https://gpuweb.github.io/gpuweb/#dom-gpudevice-createrenderpipeline
    fn CreateRenderPipeline(
        &self,
        descriptor: &GPURenderPipelineDescriptor,
    ) -> DomRoot<GPURenderPipeline> {
        let vertex_module = descriptor.vertexStage.module.id().0;
        let vertex_entry_point = descriptor.vertexStage.entryPoint.to_string();
        let (fragment_module, fragment_entry_point) = match descriptor.fragmentStage {
            Some(ref frag) => (Some(frag.module.id().0), Some(frag.entryPoint.to_string())),
            None => (None, None),
        };

        let primitive_topology = match descriptor.primitiveTopology {
            GPUPrimitiveTopology::Point_list => wgt::PrimitiveTopology::PointList,
            GPUPrimitiveTopology::Line_list => wgt::PrimitiveTopology::LineList,
            GPUPrimitiveTopology::Line_strip => wgt::PrimitiveTopology::LineStrip,
            GPUPrimitiveTopology::Triangle_list => wgt::PrimitiveTopology::TriangleList,
            GPUPrimitiveTopology::Triangle_strip => wgt::PrimitiveTopology::TriangleStrip,
        };

        let ref rs_desc = descriptor.rasterizationState;
        let rasterization_state = wgt::RasterizationStateDescriptor {
            front_face: match rs_desc.frontFace {
                GPUFrontFace::Ccw => wgt::FrontFace::Ccw,
                GPUFrontFace::Cw => wgt::FrontFace::Cw,
            },
            cull_mode: match rs_desc.cullMode {
                GPUCullMode::None => wgt::CullMode::None,
                GPUCullMode::Front => wgt::CullMode::Front,
                GPUCullMode::Back => wgt::CullMode::Back,
            },
            depth_bias: rs_desc.depthBias,
            depth_bias_slope_scale: *rs_desc.depthBiasSlopeScale,
            depth_bias_clamp: *rs_desc.depthBiasClamp,
        };

        let color_states = descriptor
            .colorStates
            .iter()
            .map(|state| wgt::ColorStateDescriptor {
                format: convert_texture_format(state.format),
                alpha_blend: convert_blend_descriptor(&state.alphaBlend),
                color_blend: convert_blend_descriptor(&state.colorBlend),
                write_mask: match wgt::ColorWrite::from_bits(state.writeMask) {
                    Some(mask) => mask,
                    None => wgt::ColorWrite::empty(),
                },
            })
            .collect::<ArrayVec<_>>();

        let depth_stencil_state = if let Some(ref dss_desc) = descriptor.depthStencilState {
            Some(wgt::DepthStencilStateDescriptor {
                format: convert_texture_format(dss_desc.format),
                depth_write_enabled: dss_desc.depthWriteEnabled,
                depth_compare: convert_compare_function(dss_desc.depthCompare),
                stencil_front: wgt::StencilStateFaceDescriptor {
                    compare: convert_compare_function(dss_desc.stencilFront.compare),
                    fail_op: convert_stencil_op(dss_desc.stencilFront.failOp),
                    depth_fail_op: convert_stencil_op(dss_desc.stencilFront.depthFailOp),
                    pass_op: convert_stencil_op(dss_desc.stencilFront.passOp),
                },
                stencil_back: wgt::StencilStateFaceDescriptor {
                    compare: convert_compare_function(dss_desc.stencilBack.compare),
                    fail_op: convert_stencil_op(dss_desc.stencilBack.failOp),
                    depth_fail_op: convert_stencil_op(dss_desc.stencilBack.depthFailOp),
                    pass_op: convert_stencil_op(dss_desc.stencilBack.passOp),
                },
                stencil_read_mask: dss_desc.stencilReadMask,
                stencil_write_mask: dss_desc.stencilWriteMask,
            })
        } else {
            None
        };

        let ref vs_desc = descriptor.vertexState;
        let vertex_state = (
            match vs_desc.indexFormat {
                GPUIndexFormat::Uint16 => wgt::IndexFormat::Uint16,
                GPUIndexFormat::Uint32 => wgt::IndexFormat::Uint32,
            },
            vs_desc
                .vertexBuffers
                .iter()
                .map(|buffer| {
                    (
                        buffer.arrayStride,
                        match buffer.stepMode {
                            GPUInputStepMode::Vertex => wgt::InputStepMode::Vertex,
                            GPUInputStepMode::Instance => wgt::InputStepMode::Instance,
                        },
                        buffer
                            .attributes
                            .iter()
                            .map(|att| wgt::VertexAttributeDescriptor {
                                format: convert_vertex_format(att.format),
                                offset: att.offset,
                                shader_location: att.shaderLocation,
                            })
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>(),
        );

        let render_pipeline_id = self
            .global()
            .wgpu_id_hub()
            .lock()
            .create_render_pipeline_id(self.device.0.backend());

        let scope_id = self.use_current_scope();

        self.channel
            .0
            .send(WebGPURequest::CreateRenderPipeline {
                device_id: self.device.0,
                render_pipeline_id,
                scope_id,
                pipeline_layout_id: descriptor.parent.layout.id().0,
                vertex_module,
                vertex_entry_point,
                fragment_module,
                fragment_entry_point,
                primitive_topology,
                rasterization_state,
                color_states,
                depth_stencil_state,
                vertex_state,
                sample_count: descriptor.sampleCount,
                sample_mask: descriptor.sampleMask,
                alpha_to_coverage_enabled: descriptor.alphaToCoverageEnabled,
            })
            .expect("Failed to create WebGPU render pipeline");

        let render_pipeline = webgpu::WebGPURenderPipeline(render_pipeline_id);

        GPURenderPipeline::new(&self.global(), render_pipeline, self.device)
    }

    /// https://gpuweb.github.io/gpuweb/#dom-gpudevice-pusherrorscope
    fn PushErrorScope(&self, filter: GPUErrorFilter) {
        let mut context = self.scope_context.borrow_mut();
        let scope_id = context.next_scope_id;
        context.next_scope_id += 1;
        let err_scope = ErrorScopeInfo {
            filter,
            op_count: 0,
            error: None,
            promise: None,
        };
        let res = context.error_scopes.insert(scope_id, err_scope);
        context.scope_stack.push(scope_id);
        assert!(res.is_none());
    }

    /// https://gpuweb.github.io/gpuweb/#dom-gpudevice-poperrorscope
    fn PopErrorScope(&self, comp: InRealm) -> Rc<Promise> {
        let mut context = self.scope_context.borrow_mut();
        let promise = Promise::new_in_current_realm(&self.global(), comp);
        let scope_id = if let Some(e) = context.scope_stack.pop() {
            e
        } else {
            promise.reject_error(Error::Operation);
            return promise;
        };
        let remove = if let Some(mut err_scope) = context.error_scopes.get_mut(&scope_id) {
            if let Some(ref e) = err_scope.error {
                match e {
                    GPUError::GPUValidationError(ref v) => promise.resolve_native(&v),
                    GPUError::GPUOutOfMemoryError(ref w) => promise.resolve_native(&w),
                }
            } else if err_scope.op_count == 0 {
                promise.resolve_native(&None::<GPUError>);
            }
            err_scope.promise = Some(promise.clone());
            err_scope.op_count == 0
        } else {
            error!("Could not find ErrorScope with Id({})", scope_id);
            false
        };
        if remove {
            let _ = context.error_scopes.remove(&scope_id);
        }
        promise
    }
}

fn convert_address_mode(address_mode: GPUAddressMode) -> wgt::AddressMode {
    match address_mode {
        GPUAddressMode::Clamp_to_edge => wgt::AddressMode::ClampToEdge,
        GPUAddressMode::Repeat => wgt::AddressMode::Repeat,
        GPUAddressMode::Mirror_repeat => wgt::AddressMode::MirrorRepeat,
    }
}

fn convert_filter_mode(filter_mode: GPUFilterMode) -> wgt::FilterMode {
    match filter_mode {
        GPUFilterMode::Nearest => wgt::FilterMode::Nearest,
        GPUFilterMode::Linear => wgt::FilterMode::Linear,
    }
}

fn convert_compare_function(compare: GPUCompareFunction) -> wgt::CompareFunction {
    match compare {
        GPUCompareFunction::Never => wgt::CompareFunction::Never,
        GPUCompareFunction::Less => wgt::CompareFunction::Less,
        GPUCompareFunction::Equal => wgt::CompareFunction::Equal,
        GPUCompareFunction::Less_equal => wgt::CompareFunction::LessEqual,
        GPUCompareFunction::Greater => wgt::CompareFunction::Greater,
        GPUCompareFunction::Not_equal => wgt::CompareFunction::NotEqual,
        GPUCompareFunction::Greater_equal => wgt::CompareFunction::GreaterEqual,
        GPUCompareFunction::Always => wgt::CompareFunction::Always,
    }
}

fn convert_blend_descriptor(desc: &GPUBlendDescriptor) -> wgt::BlendDescriptor {
    wgt::BlendDescriptor {
        src_factor: convert_blend_factor(desc.srcFactor),
        dst_factor: convert_blend_factor(desc.dstFactor),
        operation: match desc.operation {
            GPUBlendOperation::Add => wgt::BlendOperation::Add,
            GPUBlendOperation::Subtract => wgt::BlendOperation::Subtract,
            GPUBlendOperation::Reverse_subtract => wgt::BlendOperation::ReverseSubtract,
            GPUBlendOperation::Min => wgt::BlendOperation::Min,
            GPUBlendOperation::Max => wgt::BlendOperation::Max,
        },
    }
}

fn convert_blend_factor(factor: GPUBlendFactor) -> wgt::BlendFactor {
    match factor {
        GPUBlendFactor::Zero => wgt::BlendFactor::Zero,
        GPUBlendFactor::One => wgt::BlendFactor::One,
        GPUBlendFactor::Src_color => wgt::BlendFactor::SrcColor,
        GPUBlendFactor::One_minus_src_color => wgt::BlendFactor::OneMinusSrcColor,
        GPUBlendFactor::Src_alpha => wgt::BlendFactor::SrcAlpha,
        GPUBlendFactor::One_minus_src_alpha => wgt::BlendFactor::OneMinusSrcAlpha,
        GPUBlendFactor::Dst_color => wgt::BlendFactor::DstColor,
        GPUBlendFactor::One_minus_dst_color => wgt::BlendFactor::OneMinusDstColor,
        GPUBlendFactor::Dst_alpha => wgt::BlendFactor::DstAlpha,
        GPUBlendFactor::One_minus_dst_alpha => wgt::BlendFactor::OneMinusDstAlpha,
        GPUBlendFactor::Src_alpha_saturated => wgt::BlendFactor::SrcAlphaSaturated,
        GPUBlendFactor::Blend_color => wgt::BlendFactor::BlendColor,
        GPUBlendFactor::One_minus_blend_color => wgt::BlendFactor::OneMinusBlendColor,
    }
}

fn convert_stencil_op(operation: GPUStencilOperation) -> wgt::StencilOperation {
    match operation {
        GPUStencilOperation::Keep => wgt::StencilOperation::Keep,
        GPUStencilOperation::Zero => wgt::StencilOperation::Zero,
        GPUStencilOperation::Replace => wgt::StencilOperation::Replace,
        GPUStencilOperation::Invert => wgt::StencilOperation::Invert,
        GPUStencilOperation::Increment_clamp => wgt::StencilOperation::IncrementClamp,
        GPUStencilOperation::Decrement_clamp => wgt::StencilOperation::DecrementClamp,
        GPUStencilOperation::Increment_wrap => wgt::StencilOperation::IncrementWrap,
        GPUStencilOperation::Decrement_wrap => wgt::StencilOperation::DecrementWrap,
    }
}

fn convert_vertex_format(format: GPUVertexFormat) -> wgt::VertexFormat {
    match format {
        GPUVertexFormat::Uchar2 => wgt::VertexFormat::Uchar2,
        GPUVertexFormat::Uchar4 => wgt::VertexFormat::Uchar4,
        GPUVertexFormat::Char2 => wgt::VertexFormat::Char2,
        GPUVertexFormat::Char4 => wgt::VertexFormat::Char4,
        GPUVertexFormat::Uchar2norm => wgt::VertexFormat::Uchar2Norm,
        GPUVertexFormat::Uchar4norm => wgt::VertexFormat::Uchar4Norm,
        GPUVertexFormat::Char2norm => wgt::VertexFormat::Char2Norm,
        GPUVertexFormat::Char4norm => wgt::VertexFormat::Char4Norm,
        GPUVertexFormat::Ushort2 => wgt::VertexFormat::Ushort2,
        GPUVertexFormat::Ushort4 => wgt::VertexFormat::Ushort4,
        GPUVertexFormat::Short2 => wgt::VertexFormat::Short2,
        GPUVertexFormat::Short4 => wgt::VertexFormat::Short4,
        GPUVertexFormat::Ushort2norm => wgt::VertexFormat::Ushort2Norm,
        GPUVertexFormat::Ushort4norm => wgt::VertexFormat::Ushort4Norm,
        GPUVertexFormat::Short2norm => wgt::VertexFormat::Short2Norm,
        GPUVertexFormat::Short4norm => wgt::VertexFormat::Short4Norm,
        GPUVertexFormat::Half2 => wgt::VertexFormat::Half2,
        GPUVertexFormat::Half4 => wgt::VertexFormat::Half4,
        GPUVertexFormat::Float => wgt::VertexFormat::Float,
        GPUVertexFormat::Float2 => wgt::VertexFormat::Float2,
        GPUVertexFormat::Float3 => wgt::VertexFormat::Float3,
        GPUVertexFormat::Float4 => wgt::VertexFormat::Float4,
        GPUVertexFormat::Uint => wgt::VertexFormat::Uint,
        GPUVertexFormat::Uint2 => wgt::VertexFormat::Uint2,
        GPUVertexFormat::Uint3 => wgt::VertexFormat::Uint3,
        GPUVertexFormat::Uint4 => wgt::VertexFormat::Uint4,
        GPUVertexFormat::Int => wgt::VertexFormat::Int,
        GPUVertexFormat::Int2 => wgt::VertexFormat::Int2,
        GPUVertexFormat::Int3 => wgt::VertexFormat::Int3,
        GPUVertexFormat::Int4 => wgt::VertexFormat::Int4,
    }
}

pub fn convert_texture_format(format: GPUTextureFormat) -> wgt::TextureFormat {
    match format {
        GPUTextureFormat::R8unorm => wgt::TextureFormat::R8Unorm,
        GPUTextureFormat::R8snorm => wgt::TextureFormat::R8Snorm,
        GPUTextureFormat::R8uint => wgt::TextureFormat::R8Uint,
        GPUTextureFormat::R8sint => wgt::TextureFormat::R8Sint,
        GPUTextureFormat::R16uint => wgt::TextureFormat::R16Uint,
        GPUTextureFormat::R16sint => wgt::TextureFormat::R16Sint,
        GPUTextureFormat::R16float => wgt::TextureFormat::R16Float,
        GPUTextureFormat::Rg8unorm => wgt::TextureFormat::Rg8Unorm,
        GPUTextureFormat::Rg8snorm => wgt::TextureFormat::Rg8Snorm,
        GPUTextureFormat::Rg8uint => wgt::TextureFormat::Rg8Uint,
        GPUTextureFormat::Rg8sint => wgt::TextureFormat::Rg8Sint,
        GPUTextureFormat::R32uint => wgt::TextureFormat::R32Uint,
        GPUTextureFormat::R32sint => wgt::TextureFormat::R32Sint,
        GPUTextureFormat::R32float => wgt::TextureFormat::R32Float,
        GPUTextureFormat::Rg16uint => wgt::TextureFormat::Rg16Uint,
        GPUTextureFormat::Rg16sint => wgt::TextureFormat::Rg16Sint,
        GPUTextureFormat::Rg16float => wgt::TextureFormat::Rg16Float,
        GPUTextureFormat::Rgba8unorm => wgt::TextureFormat::Rgba8Unorm,
        GPUTextureFormat::Rgba8unorm_srgb => wgt::TextureFormat::Rgba8UnormSrgb,
        GPUTextureFormat::Rgba8snorm => wgt::TextureFormat::Rgba8Snorm,
        GPUTextureFormat::Rgba8uint => wgt::TextureFormat::Rgba8Uint,
        GPUTextureFormat::Rgba8sint => wgt::TextureFormat::Rgba8Sint,
        GPUTextureFormat::Bgra8unorm => wgt::TextureFormat::Bgra8Unorm,
        GPUTextureFormat::Bgra8unorm_srgb => wgt::TextureFormat::Bgra8UnormSrgb,
        GPUTextureFormat::Rgb10a2unorm => wgt::TextureFormat::Rgb10a2Unorm,
        GPUTextureFormat::Rg11b10float => wgt::TextureFormat::Rg11b10Float,
        GPUTextureFormat::Rg32uint => wgt::TextureFormat::Rg32Uint,
        GPUTextureFormat::Rg32sint => wgt::TextureFormat::Rg32Sint,
        GPUTextureFormat::Rg32float => wgt::TextureFormat::Rg32Float,
        GPUTextureFormat::Rgba16uint => wgt::TextureFormat::Rgba16Uint,
        GPUTextureFormat::Rgba16sint => wgt::TextureFormat::Rgba16Sint,
        GPUTextureFormat::Rgba16float => wgt::TextureFormat::Rgba16Float,
        GPUTextureFormat::Rgba32uint => wgt::TextureFormat::Rgba32Uint,
        GPUTextureFormat::Rgba32sint => wgt::TextureFormat::Rgba32Sint,
        GPUTextureFormat::Rgba32float => wgt::TextureFormat::Rgba32Float,
        GPUTextureFormat::Depth32float => wgt::TextureFormat::Depth32Float,
        GPUTextureFormat::Depth24plus => wgt::TextureFormat::Depth24Plus,
        GPUTextureFormat::Depth24plus_stencil8 => wgt::TextureFormat::Depth24PlusStencil8,
    }
}

pub fn convert_texture_view_dimension(
    dimension: GPUTextureViewDimension,
) -> wgt::TextureViewDimension {
    match dimension {
        GPUTextureViewDimension::_1d => wgt::TextureViewDimension::D1,
        GPUTextureViewDimension::_2d => wgt::TextureViewDimension::D2,
        GPUTextureViewDimension::_2d_array => wgt::TextureViewDimension::D2Array,
        GPUTextureViewDimension::Cube => wgt::TextureViewDimension::Cube,
        GPUTextureViewDimension::Cube_array => wgt::TextureViewDimension::CubeArray,
        GPUTextureViewDimension::_3d => wgt::TextureViewDimension::D3,
    }
}

pub fn convert_texture_size_to_dict(size: &GPUExtent3D) -> GPUExtent3DDict {
    match *size {
        GPUExtent3D::GPUExtent3DDict(ref dict) => GPUExtent3DDict {
            width: dict.width,
            height: dict.height,
            depth: dict.depth,
        },
        GPUExtent3D::RangeEnforcedUnsignedLongSequence(ref v) => GPUExtent3DDict {
            width: v[0],
            height: v[1],
            depth: v[2],
        },
    }
}

pub fn convert_texture_size_to_wgt(size: &GPUExtent3DDict) -> wgt::Extent3d {
    wgt::Extent3d {
        width: size.width,
        height: size.height,
        depth: size.depth,
    }
}
