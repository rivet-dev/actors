//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

using MonoFN.Cecil.Cil;
using MonoFN.Cecil.Metadata;
using MonoFN.Cecil.PE;
using MonoFN.Collections.Generic;
using System;
using System.Collections.Generic;
using System.IO;
using System.IO.Compression;
using System.Text;
using RVA = System.UInt32;

namespace MonoFN.Cecil
{
    internal abstract class ModuleReader
    {
        protected readonly ModuleDefinition module;

        protected ModuleReader(Image image, ReadingMode mode)
        {
            module = new(image);
            module.ReadingMode = mode;
        }

        protected abstract void ReadModule();
        public abstract void ReadSymbols(ModuleDefinition module);

        protected void ReadModuleManifest(MetadataReader reader)
        {
            reader.Populate(module);

            ReadAssembly(reader);
        }

        private void ReadAssembly(MetadataReader reader)
        {
            AssemblyNameDefinition name = reader.ReadAssemblyNameDefinition();
            if (name == null)
            {
                module.kind = ModuleKind.NetModule;
                return;
            }

            AssemblyDefinition assembly = new();
            assembly.Name = name;

            module.assembly = assembly;
            assembly.main_module = module;
        }

        public static ModuleDefinition CreateModule(Image image, ReaderParameters parameters)
        {
            ModuleReader reader = CreateModuleReader(image, parameters.ReadingMode);
            ModuleDefinition module = reader.module;

            if (parameters.assembly_resolver != null)
                module.assembly_resolver = Disposable.NotOwned(parameters.assembly_resolver);

            if (parameters.metadata_resolver != null)
                module.metadata_resolver = parameters.metadata_resolver;

            if (parameters.metadata_importer_provider != null)
                module.metadata_importer = parameters.metadata_importer_provider.GetMetadataImporter(module);

            if (parameters.reflection_importer_provider != null)
                module.reflection_importer = parameters.reflection_importer_provider.GetReflectionImporter(module);

            GetMetadataKind(module, parameters);

            reader.ReadModule();

            ReadSymbols(module, parameters);

            reader.ReadSymbols(module);

            if (parameters.ReadingMode == ReadingMode.Immediate)
                module.MetadataSystem.Clear();

            return module;
        }

        private static void ReadSymbols(ModuleDefinition module, ReaderParameters parameters)
        {
            ISymbolReaderProvider symbol_reader_provider = parameters.SymbolReaderProvider;

            if (symbol_reader_provider == null && parameters.ReadSymbols)
                symbol_reader_provider = new DefaultSymbolReaderProvider();

            if (symbol_reader_provider != null)
            {
                module.SymbolReaderProvider = symbol_reader_provider;

                ISymbolReader reader = parameters.SymbolStream != null ? symbol_reader_provider.GetSymbolReader(module, parameters.SymbolStream) : symbol_reader_provider.GetSymbolReader(module, module.FileName);

                if (reader != null)
                {
                    try
                    {
                        module.ReadSymbols(reader, parameters.ThrowIfSymbolsAreNotMatching);
                    }
                    catch (Exception)
                    {
                        reader.Dispose();
                        throw;
                    }
                }
            }

            if (module.Image.HasDebugTables())
                module.ReadSymbols(new PortablePdbReader(module.Image, module));
        }

        private static void GetMetadataKind(ModuleDefinition module, ReaderParameters parameters)
        {
            if (!parameters.ApplyWindowsRuntimeProjections)
            {
                module.MetadataKind = MetadataKind.Ecma335;
                return;
            }

            string runtime_version = module.RuntimeVersion;

            if (!runtime_version.Contains("WindowsRuntime"))
                module.MetadataKind = MetadataKind.Ecma335;
            else if (runtime_version.Contains("CLR"))
                module.MetadataKind = MetadataKind.ManagedWindowsMetadata;
            else
                module.MetadataKind = MetadataKind.WindowsMetadata;
        }

        private static ModuleReader CreateModuleReader(Image image, ReadingMode mode)
        {
            switch (mode)
            {
                case ReadingMode.Immediate:
                    return new ImmediateModuleReader(image);
                case ReadingMode.Deferred:
                    return new DeferredModuleReader(image);
                default:
                    throw new ArgumentException();
            }
        }
    }

    internal sealed class ImmediateModuleReader : ModuleReader
    {
        private bool resolve_attributes;
        public ImmediateModuleReader(Image image) : base(image, ReadingMode.Immediate) { }

        protected override void ReadModule()
        {
            this.module.Read(this.module, (module, reader) =>
            {
                ReadModuleManifest(reader);
                ReadModule(module, resolve_attributes: true);
            });
        }

        public void ReadModule(ModuleDefinition module, bool resolve_attributes)
        {
            this.resolve_attributes = resolve_attributes;

            if (module.HasAssemblyReferences)
                Mixin.Read(module.AssemblyReferences);
            if (module.HasResources)
                Mixin.Read(module.Resources);
            if (module.HasModuleReferences)
                Mixin.Read(module.ModuleReferences);
            if (module.HasTypes)
                ReadTypes(module.Types);
            if (module.HasExportedTypes)
                Mixin.Read(module.ExportedTypes);

            ReadCustomAttributes(module);

            AssemblyDefinition assembly = module.Assembly;
            if (module.kind == ModuleKind.NetModule || assembly == null)
                return;

            ReadCustomAttributes(assembly);
            ReadSecurityDeclarations(assembly);
        }

        private void ReadTypes(Collection<TypeDefinition> types)
        {
            for (int i = 0; i < types.Count; i++)
                ReadType(types[i]);
        }

        private void ReadType(TypeDefinition type)
        {
            ReadGenericParameters(type);

            if (type.HasInterfaces)
                ReadInterfaces(type);

            if (type.HasNestedTypes)
                ReadTypes(type.NestedTypes);

            if (type.HasLayoutInfo)
                Mixin.Read(type.ClassSize);

            if (type.HasFields)
                ReadFields(type);

            if (type.HasMethods)
                ReadMethods(type);

            if (type.HasProperties)
                ReadProperties(type);

            if (type.HasEvents)
                ReadEvents(type);

            ReadSecurityDeclarations(type);
            ReadCustomAttributes(type);
        }

        private void ReadInterfaces(TypeDefinition type)
        {
            Collection<InterfaceImplementation> interfaces = type.Interfaces;

            for (int i = 0; i < interfaces.Count; i++)
                ReadCustomAttributes(interfaces[i]);
        }

        private void ReadGenericParameters(IGenericParameterProvider provider)
        {
            if (!provider.HasGenericParameters)
                return;

            Collection<GenericParameter> parameters = provider.GenericParameters;

            for (int i = 0; i < parameters.Count; i++)
            {
                GenericParameter parameter = parameters[i];

                if (parameter.HasConstraints)
                    ReadGenericParameterConstraints(parameter);

                ReadCustomAttributes(parameter);
            }
        }

        private void ReadGenericParameterConstraints(GenericParameter parameter)
        {
            Collection<GenericParameterConstraint> constraints = parameter.Constraints;

            for (int i = 0; i < constraints.Count; i++)
                ReadCustomAttributes(constraints[i]);
        }

        private void ReadSecurityDeclarations(ISecurityDeclarationProvider provider)
        {
            if (!provider.HasSecurityDeclarations)
                return;

            Collection<SecurityDeclaration> security_declarations = provider.SecurityDeclarations;

            if (!resolve_attributes)
                return;

            for (int i = 0; i < security_declarations.Count; i++)
            {
                SecurityDeclaration security_declaration = security_declarations[i];

                Mixin.Read(security_declaration.SecurityAttributes);
            }
        }

        private void ReadCustomAttributes(ICustomAttributeProvider provider)
        {
            if (!provider.HasCustomAttributes)
                return;

            Collection<CustomAttribute> custom_attributes = provider.CustomAttributes;

            if (!resolve_attributes)
                return;

            for (int i = 0; i < custom_attributes.Count; i++)
            {
                CustomAttribute custom_attribute = custom_attributes[i];

                Mixin.Read(custom_attribute.ConstructorArguments);
            }
        }

        private void ReadFields(TypeDefinition type)
        {
            Collection<FieldDefinition> fields = type.Fields;

            for (int i = 0; i < fields.Count; i++)
            {
                FieldDefinition field = fields[i];

                if (field.HasConstant)
                    Mixin.Read(field.Constant);

                if (field.HasLayoutInfo)
                    Mixin.Read(field.Offset);

                if (field.RVA > 0)
                    Mixin.Read(field.InitialValue);

                if (field.HasMarshalInfo)
                    Mixin.Read(field.MarshalInfo);

                ReadCustomAttributes(field);
            }
        }

        private void ReadMethods(TypeDefinition type)
        {
            Collection<MethodDefinition> methods = type.Methods;

            for (int i = 0; i < methods.Count; i++)
            {
                MethodDefinition method = methods[i];

                ReadGenericParameters(method);

                if (method.HasParameters)
                    ReadParameters(method);

                if (method.HasOverrides)
                    Mixin.Read(method.Overrides);

                if (method.IsPInvokeImpl)
                    Mixin.Read(method.PInvokeInfo);

                ReadSecurityDeclarations(method);
                ReadCustomAttributes(method);

                MethodReturnType return_type = method.MethodReturnType;
                if (return_type.HasConstant)
                    Mixin.Read(return_type.Constant);

                if (return_type.HasMarshalInfo)
                    Mixin.Read(return_type.MarshalInfo);

                ReadCustomAttributes(return_type);
            }
        }

        private void ReadParameters(MethodDefinition method)
        {
            Collection<ParameterDefinition> parameters = method.Parameters;

            for (int i = 0; i < parameters.Count; i++)
            {
                ParameterDefinition parameter = parameters[i];

                if (parameter.HasConstant)
                    Mixin.Read(parameter.Constant);

                if (parameter.HasMarshalInfo)
                    Mixin.Read(parameter.MarshalInfo);

                ReadCustomAttributes(parameter);
            }
        }

        private void ReadProperties(TypeDefinition type)
        {
            Collection<PropertyDefinition> properties = type.Properties;

            for (int i = 0; i < properties.Count; i++)
            {
                PropertyDefinition property = properties[i];

                Mixin.Read(property.GetMethod);

                if (property.HasConstant)
                    Mixin.Read(property.Constant);

                ReadCustomAttributes(property);
            }
        }

        private void ReadEvents(TypeDefinition type)
        {
            Collection<EventDefinition> events = type.Events;

            for (int i = 0; i < events.Count; i++)
            {
                EventDefinition @event = events[i];

                Mixin.Read(@event.AddMethod);

                ReadCustomAttributes(@event);
            }
        }

        public override void ReadSymbols(ModuleDefinition module)
        {
            if (module.symbol_reader == null)
                return;

            ReadTypesSymbols(module.Types, module.symbol_reader);
        }

        private void ReadTypesSymbols(Collection<TypeDefinition> types, ISymbolReader symbol_reader)
        {
            for (int i = 0; i < types.Count; i++)
            {
                TypeDefinition type = types[i];

                if (type.HasNestedTypes)
                    ReadTypesSymbols(type.NestedTypes, symbol_reader);

                if (type.HasMethods)
                    ReadMethodsSymbols(type, symbol_reader);
            }
        }

        private void ReadMethodsSymbols(TypeDefinition type, ISymbolReader symbol_reader)
        {
            Collection<MethodDefinition> methods = type.Methods;
            for (int i = 0; i < methods.Count; i++)
            {
                MethodDefinition method = methods[i];

                if (method.HasBody && method.token.RID != 0 && method.debug_info == null)
                    method.debug_info = symbol_reader.Read(method);
            }
        }
    }

    internal sealed class DeferredModuleReader : ModuleReader
    {
        public DeferredModuleReader(Image image) : base(image, ReadingMode.Deferred) { }

        protected override void ReadModule()
        {
            module.Read(module, (_, reader) => ReadModuleManifest(reader));
        }

        public override void ReadSymbols(ModuleDefinition module) { }
    }

    internal sealed class MetadataReader : ByteBuffer
    {
        internal readonly Image image;
        internal readonly ModuleDefinition module;
        internal readonly MetadataSystem metadata;
        internal CodeReader code;
        internal IGenericContext context;
        private readonly MetadataReader metadata_reader;

        public MetadataReader(ModuleDefinition module) : base(module.Image.TableHeap.data)
        {
            image = module.Image;
            this.module = module;
            metadata = module.MetadataSystem;
            code = new(this);
        }

        public MetadataReader(Image image, ModuleDefinition module, MetadataReader metadata_reader) : base(image.TableHeap.data)
        {
            this.image = image;
            this.module = module;
            metadata = module.MetadataSystem;
            this.metadata_reader = metadata_reader;
        }

        private int GetCodedIndexSize(CodedIndex index)
        {
            return image.GetCodedIndexSize(index);
        }

        private uint ReadByIndexSize(int size)
        {
            if (size == 4)
                return ReadUInt32();
            else
                return ReadUInt16();
        }

        private byte[] ReadBlob()
        {
            BlobHeap blob_heap = image.BlobHeap;
            if (blob_heap == null)
            {
                position += 2;
                return Empty<byte>.Array;
            }

            return blob_heap.Read(ReadBlobIndex());
        }

        private byte[] ReadBlob(uint signature)
        {
            BlobHeap blob_heap = image.BlobHeap;
            if (blob_heap == null)
                return Empty<byte>.Array;

            return blob_heap.Read(signature);
        }

        private uint ReadBlobIndex()
        {
            BlobHeap blob_heap = image.BlobHeap;
            return ReadByIndexSize(blob_heap != null ? blob_heap.IndexSize : 2);
        }

        private void GetBlobView(uint signature, out byte[] blob, out int index, out int count)
        {
            BlobHeap blob_heap = image.BlobHeap;
            if (blob_heap == null)
            {
                blob = null;
                index = count = 0;
                return;
            }

            blob_heap.GetView(signature, out blob, out index, out count);
        }

        private string ReadString()
        {
            return image.StringHeap.Read(ReadByIndexSize(image.StringHeap.IndexSize));
        }

        private uint ReadStringIndex()
        {
            return ReadByIndexSize(image.StringHeap.IndexSize);
        }

        private Guid ReadGuid()
        {
            return image.GuidHeap.Read(ReadByIndexSize(image.GuidHeap.IndexSize));
        }

        private uint ReadTableIndex(Table table)
        {
            return ReadByIndexSize(image.GetTableIndexSize(table));
        }

        private MetadataToken ReadMetadataToken(CodedIndex index)
        {
            return index.GetMetadataToken(ReadByIndexSize(GetCodedIndexSize(index)));
        }

        private int MoveTo(Table table)
        {
            TableInformation info = image.TableHeap[table];
            if (info.Length != 0)
                position = (int)info.Offset;

            return (int)info.Length;
        }

        private bool MoveTo(Table table, uint row)
        {
            TableInformation info = image.TableHeap[table];
            uint length = info.Length;
            if (length == 0 || row > length)
                return false;

            position = (int)(info.Offset + info.RowSize * (row - 1));
            return true;
        }

        public AssemblyNameDefinition ReadAssemblyNameDefinition()
        {
            if (MoveTo(Table.Assembly) == 0)
                return null;

            AssemblyNameDefinition name = new();

            name.HashAlgorithm = (AssemblyHashAlgorithm)ReadUInt32();

            PopulateVersionAndFlags(name);

            name.PublicKey = ReadBlob();

            PopulateNameAndCulture(name);

            return name;
        }

        public ModuleDefinition Populate(ModuleDefinition module)
        {
            if (MoveTo(Table.Module) == 0)
                return module;

            Advance(2); // Generation

            module.Name = ReadString();
            module.Mvid = ReadGuid();

            return module;
        }

        private void InitializeAssemblyReferences()
        {
            if (metadata.AssemblyReferences != null)
                return;

            int length = MoveTo(Table.AssemblyRef);
            AssemblyNameReference[] references = metadata.AssemblyReferences = new AssemblyNameReference [length];

            for (uint i = 0; i < length; i++)
            {
                AssemblyNameReference reference = new();
                reference.token = new(TokenType.AssemblyRef, i + 1);

                PopulateVersionAndFlags(reference);

                byte[] key_or_token = ReadBlob();

                if (reference.HasPublicKey)
                    reference.PublicKey = key_or_token;
                else
                    reference.PublicKeyToken = key_or_token;

                PopulateNameAndCulture(reference);

                reference.Hash = ReadBlob();

                references[i] = reference;
            }
        }

        public Collection<AssemblyNameReference> ReadAssemblyReferences()
        {
            InitializeAssemblyReferences();

            Collection<AssemblyNameReference> references = new(metadata.AssemblyReferences);
            if (module.IsWindowsMetadata())
                module.Projections.AddVirtualReferences(references);

            return references;
        }

        public MethodDefinition ReadEntryPoint()
        {
            if (module.Image.EntryPointToken == 0)
                return null;

            MetadataToken token = new(module.Image.EntryPointToken);
            return GetMethodDefinition(token.RID);
        }

        public Collection<ModuleDefinition> ReadModules()
        {
            Collection<ModuleDefinition> modules = new(1);
            modules.Add(module);

            int length = MoveTo(Table.File);
            for (uint i = 1; i <= length; i++)
            {
                FileAttributes attributes = (FileAttributes)ReadUInt32();
                string name = ReadString();
                ReadBlobIndex();

                if (attributes != FileAttributes.ContainsMetaData)
                    continue;

                ReaderParameters parameters = new()
                {
                    ReadingMode = module.ReadingMode,
                    SymbolReaderProvider = module.SymbolReaderProvider,
                    AssemblyResolver = module.AssemblyResolver
                };

                ModuleDefinition netmodule = ModuleDefinition.ReadModule(GetModuleFileName(name), parameters);
                netmodule.assembly = module.assembly;

                modules.Add(netmodule);
            }

            return modules;
        }

        private string GetModuleFileName(string name)
        {
            if (module.FileName == null)
                throw new NotSupportedException();

            string path = Path.GetDirectoryName(module.FileName);
            return Path.Combine(path, name);
        }

        private void InitializeModuleReferences()
        {
            if (metadata.ModuleReferences != null)
                return;

            int length = MoveTo(Table.ModuleRef);
            ModuleReference[] references = metadata.ModuleReferences = new ModuleReference [length];

            for (uint i = 0; i < length; i++)
            {
                ModuleReference reference = new(ReadString());
                reference.token = new(TokenType.ModuleRef, i + 1);

                references[i] = reference;
            }
        }

        public Collection<ModuleReference> ReadModuleReferences()
        {
            InitializeModuleReferences();

            return new(metadata.ModuleReferences);
        }

        public bool HasFileResource()
        {
            int length = MoveTo(Table.File);
            if (length == 0)
                return false;

            for (uint i = 1; i <= length; i++)
                if (ReadFileRecord(i).Col1 == FileAttributes.ContainsNoMetaData)
                    return true;

            return false;
        }

        public Collection<Resource> ReadResources()
        {
            int length = MoveTo(Table.ManifestResource);
            Collection<Resource> resources = new(length);

            for (int i = 1; i <= length; i++)
            {
                uint offset = ReadUInt32();
                ManifestResourceAttributes flags = (ManifestResourceAttributes)ReadUInt32();
                string name = ReadString();
                MetadataToken implementation = ReadMetadataToken(CodedIndex.Implementation);

                Resource resource;

                if (implementation.RID == 0)
                {
                    resource = new EmbeddedResource(name, flags, offset, this);
                }
                else if (implementation.TokenType == TokenType.AssemblyRef)
                {
                    resource = new AssemblyLinkedResource(name, flags)
                    {
                        Assembly = (AssemblyNameReference)GetTypeReferenceScope(implementation)
                    };
                }
                else if (implementation.TokenType == TokenType.File)
                {
                    Row<FileAttributes, string, uint> file_record = ReadFileRecord(implementation.RID);

                    resource = new LinkedResource(name, flags)
                    {
                        File = file_record.Col2,
                        hash = ReadBlob(file_record.Col3)
                    };
                }
                else
                {
                    continue;
                }

                resources.Add(resource);
            }

            return resources;
        }

        private Row<FileAttributes, string, uint> ReadFileRecord(uint rid)
        {
            int position = this.position;

            if (!MoveTo(Table.File, rid))
                throw new ArgumentException();

            Row<FileAttributes, string, uint> record = new((FileAttributes)ReadUInt32(), ReadString(), ReadBlobIndex());

            this.position = position;

            return record;
        }

        public byte[] GetManagedResource(uint offset)
        {
            return image.GetReaderAt(image.Resources.VirtualAddress, offset, (o, reader) =>
            {
                reader.Advance((int)o);
                return reader.ReadBytes(reader.ReadInt32());
            }) ?? Empty<byte>.Array;
        }

        private void PopulateVersionAndFlags(AssemblyNameReference name)
        {
            name.Version = new(ReadUInt16(), ReadUInt16(), ReadUInt16(), ReadUInt16());

            name.Attributes = (AssemblyAttributes)ReadUInt32();
        }

        private void PopulateNameAndCulture(AssemblyNameReference name)
        {
            name.Name = ReadString();
            name.Culture = ReadString();
        }

        public TypeDefinitionCollection ReadTypes()
        {
            InitializeTypeDefinitions();
            TypeDefinition[] mtypes = metadata.Types;
            int type_count = mtypes.Length - metadata.NestedTypes.Count;
            TypeDefinitionCollection types = new(module, type_count);

            for (int i = 0; i < mtypes.Length; i++)
            {
                TypeDefinition type = mtypes[i];
                if (IsNested(type.Attributes))
                    continue;

                types.Add(type);
            }

            if (image.HasTable(Table.MethodPtr) || image.HasTable(Table.FieldPtr))
                CompleteTypes();

            return types;
        }

        private void CompleteTypes()
        {
            TypeDefinition[] types = metadata.Types;

            for (int i = 0; i < types.Length; i++)
            {
                TypeDefinition type = types[i];

                Mixin.Read(type.Fields);
                Mixin.Read(type.Methods);
            }
        }

        private void InitializeTypeDefinitions()
        {
            if (metadata.Types != null)
                return;

            InitializeNestedTypes();
            InitializeFields();
            InitializeMethods();

            int length = MoveTo(Table.TypeDef);
            TypeDefinition[] types = metadata.Types = new TypeDefinition [length];

            for (uint i = 0; i < length; i++)
            {
                if (types[i] != null)
                    continue;

                types[i] = ReadType(i + 1);
            }

            if (module.IsWindowsMetadata())
            {
                for (uint i = 0; i < length; i++)
                {
                    WindowsRuntimeProjections.Project(types[i]);
                }
            }
        }

        private static bool IsNested(TypeAttributes attributes)
        {
            switch (attributes & TypeAttributes.VisibilityMask)
            {
                case TypeAttributes.NestedAssembly:
                case TypeAttributes.NestedFamANDAssem:
                case TypeAttributes.NestedFamily:
                case TypeAttributes.NestedFamORAssem:
                case TypeAttributes.NestedPrivate:
                case TypeAttributes.NestedPublic:
                    return true;
                default:
                    return false;
            }
        }

        public bool HasNestedTypes(TypeDefinition type)
        {
            Collection<uint> mapping;
            InitializeNestedTypes();

            if (!metadata.TryGetNestedTypeMapping(type, out mapping))
                return false;

            return mapping.Count > 0;
        }

        public Collection<TypeDefinition> ReadNestedTypes(TypeDefinition type)
        {
            InitializeNestedTypes();
            Collection<uint> mapping;
            if (!metadata.TryGetNestedTypeMapping(type, out mapping))
                return new MemberDefinitionCollection<TypeDefinition>(type);

            MemberDefinitionCollection<TypeDefinition> nested_types = new(type, mapping.Count);

            for (int i = 0; i < mapping.Count; i++)
            {
                TypeDefinition nested_type = GetTypeDefinition(mapping[i]);

                if (nested_type != null)
                    nested_types.Add(nested_type);
            }

            metadata.RemoveNestedTypeMapping(type);

            return nested_types;
        }

        private void InitializeNestedTypes()
        {
            if (metadata.NestedTypes != null)
                return;

            int length = MoveTo(Table.NestedClass);

            metadata.NestedTypes = new(length);
            metadata.ReverseNestedTypes = new(length);

            if (length == 0)
                return;

            for (int i = 1; i <= length; i++)
            {
                uint nested = ReadTableIndex(Table.TypeDef);
                uint declaring = ReadTableIndex(Table.TypeDef);

                AddNestedMapping(declaring, nested);
            }
        }

        private void AddNestedMapping(uint declaring, uint nested)
        {
            metadata.SetNestedTypeMapping(declaring, AddMapping(metadata.NestedTypes, declaring, nested));
            metadata.SetReverseNestedTypeMapping(nested, declaring);
        }

        private static Collection<TValue> AddMapping<TKey, TValue>(Dictionary<TKey, Collection<TValue>> cache, TKey key, TValue value)
        {
            Collection<TValue> mapped;
            if (!cache.TryGetValue(key, out mapped))
            {
                mapped = new();
            }
            mapped.Add(value);
            return mapped;
        }

        private TypeDefinition ReadType(uint rid)
        {
            if (!MoveTo(Table.TypeDef, rid))
                return null;

            TypeAttributes attributes = (TypeAttributes)ReadUInt32();
            string name = ReadString();
            string @namespace = ReadString();
            TypeDefinition type = new(@namespace, name, attributes);
            type.token = new(TokenType.TypeDef, rid);
            type.scope = module;
            type.module = module;

            metadata.AddTypeDefinition(type);

            context = type;

            type.BaseType = GetTypeDefOrRef(ReadMetadataToken(CodedIndex.TypeDefOrRef));

            type.fields_range = ReadListRange(rid, Table.TypeDef, Table.Field);
            type.methods_range = ReadListRange(rid, Table.TypeDef, Table.Method);

            if (IsNested(attributes))
                type.DeclaringType = GetNestedTypeDeclaringType(type);

            return type;
        }

        private TypeDefinition GetNestedTypeDeclaringType(TypeDefinition type)
        {
            uint declaring_rid;
            if (!metadata.TryGetReverseNestedTypeMapping(type, out declaring_rid))
                return null;

            metadata.RemoveReverseNestedTypeMapping(type);
            return GetTypeDefinition(declaring_rid);
        }

        private Range ReadListRange(uint current_index, Table current, Table target)
        {
            Range list = new();

            uint start = ReadTableIndex(target);
            if (start == 0)
                return list;

            uint next_index;
            TableInformation current_table = image.TableHeap[current];

            if (current_index == current_table.Length)
            {
                next_index = image.TableHeap[target].Length + 1;
            }
            else
            {
                int position = this.position;
                this.position += (int)(current_table.RowSize - image.GetTableIndexSize(target));
                next_index = ReadTableIndex(target);
                this.position = position;
            }

            list.Start = start;
            list.Length = next_index - start;

            return list;
        }

        public Row<short, int> ReadTypeLayout(TypeDefinition type)
        {
            InitializeTypeLayouts();
            Row<ushort, uint> class_layout;
            uint rid = type.token.RID;
            if (!metadata.ClassLayouts.TryGetValue(rid, out class_layout))
                return new(Mixin.NoDataMarker, Mixin.NoDataMarker);

            type.PackingSize = (short)class_layout.Col1;
            type.ClassSize = (int)class_layout.Col2;

            metadata.ClassLayouts.Remove(rid);

            return new((short)class_layout.Col1, (int)class_layout.Col2);
        }

        private void InitializeTypeLayouts()
        {
            if (metadata.ClassLayouts != null)
                return;

            int length = MoveTo(Table.ClassLayout);

            Dictionary<uint, Row<ushort, uint>> class_layouts = metadata.ClassLayouts = new(length);

            for (uint i = 0; i < length; i++)
            {
                ushort packing_size = ReadUInt16();
                uint class_size = ReadUInt32();

                uint parent = ReadTableIndex(Table.TypeDef);

                class_layouts.Add(parent, new(packing_size, class_size));
            }
        }

        public TypeReference GetTypeDefOrRef(MetadataToken token)
        {
            return (TypeReference)LookupToken(token);
        }

        public TypeDefinition GetTypeDefinition(uint rid)
        {
            InitializeTypeDefinitions();

            TypeDefinition type = metadata.GetTypeDefinition(rid);
            if (type != null)
                return type;

            type = ReadTypeDefinition(rid);

            if (module.IsWindowsMetadata())
                WindowsRuntimeProjections.Project(type);

            return type;
        }

        private TypeDefinition ReadTypeDefinition(uint rid)
        {
            if (!MoveTo(Table.TypeDef, rid))
                return null;

            return ReadType(rid);
        }

        private void InitializeTypeReferences()
        {
            if (metadata.TypeReferences != null)
                return;

            metadata.TypeReferences = new TypeReference [image.GetTableLength(Table.TypeRef)];
        }

        public TypeReference GetTypeReference(string scope, string full_name)
        {
            InitializeTypeReferences();

            int length = metadata.TypeReferences.Length;

            for (uint i = 1; i <= length; i++)
            {
                TypeReference type = GetTypeReference(i);

                if (type.FullName != full_name)
                    continue;

                if (string.IsNullOrEmpty(scope))
                    return type;

                if (type.Scope.Name == scope)
                    return type;
            }

            return null;
        }

        private TypeReference GetTypeReference(uint rid)
        {
            InitializeTypeReferences();

            TypeReference type = metadata.GetTypeReference(rid);
            if (type != null)
                return type;

            return ReadTypeReference(rid);
        }

        private TypeReference ReadTypeReference(uint rid)
        {
            if (!MoveTo(Table.TypeRef, rid))
                return null;

            TypeReference declaring_type = null;
            IMetadataScope scope;

            MetadataToken scope_token = ReadMetadataToken(CodedIndex.ResolutionScope);

            string name = ReadString();
            string @namespace = ReadString();

            TypeReference type = new(@namespace, name, module, null);

            type.token = new(TokenType.TypeRef, rid);

            metadata.AddTypeReference(type);

            if (scope_token.TokenType == TokenType.TypeRef)
            {
                if (scope_token.RID != rid)
                {
                    declaring_type = GetTypeDefOrRef(scope_token);

                    scope = declaring_type != null ? declaring_type.Scope : module;
                }
                else // obfuscated typeref row pointing to self
                {
                    scope = module;
                }
            }
            else
            {
                scope = GetTypeReferenceScope(scope_token);
            }

            type.scope = scope;
            type.DeclaringType = declaring_type;

            MetadataSystem.TryProcessPrimitiveTypeReference(type);

            if (type.Module.IsWindowsMetadata())
                WindowsRuntimeProjections.Project(type);

            return type;
        }

        private IMetadataScope GetTypeReferenceScope(MetadataToken scope)
        {
            if (scope.TokenType == TokenType.Module)
                return module;

            IMetadataScope[] scopes;

            switch (scope.TokenType)
            {
                case TokenType.AssemblyRef:
                    InitializeAssemblyReferences();
                    scopes = metadata.AssemblyReferences;
                    break;
                case TokenType.ModuleRef:
                    InitializeModuleReferences();
                    scopes = metadata.ModuleReferences;
                    break;
                default:
                    throw new NotSupportedException();
            }

            uint index = scope.RID - 1;
            if (index < 0 || index >= scopes.Length)
                return null;

            return scopes[index];
        }

        public IEnumerable<TypeReference> GetTypeReferences()
        {
            InitializeTypeReferences();

            int length = image.GetTableLength(Table.TypeRef);

            TypeReference[] type_references = new TypeReference [length];

            for (uint i = 1; i <= length; i++)
                type_references[i - 1] = GetTypeReference(i);

            return type_references;
        }

        private TypeReference GetTypeSpecification(uint rid)
        {
            if (!MoveTo(Table.TypeSpec, rid))
                return null;

            SignatureReader reader = ReadSignature(ReadBlobIndex());
            TypeReference type = reader.ReadTypeSignature();
            if (type.token.RID == 0)
                type.token = new(TokenType.TypeSpec, rid);

            return type;
        }

        private SignatureReader ReadSignature(uint signature)
        {
            return new(signature, this);
        }

        public bool HasInterfaces(TypeDefinition type)
        {
            InitializeInterfaces();
            Collection<Row<uint, MetadataToken>> mapping;

            return metadata.TryGetInterfaceMapping(type, out mapping);
        }

        public InterfaceImplementationCollection ReadInterfaces(TypeDefinition type)
        {
            InitializeInterfaces();
            Collection<Row<uint, MetadataToken>> mapping;

            if (!metadata.TryGetInterfaceMapping(type, out mapping))
                return new(type);

            InterfaceImplementationCollection interfaces = new(type, mapping.Count);

            context = type;

            for (int i = 0; i < mapping.Count; i++)
            {
                interfaces.Add(new(GetTypeDefOrRef(mapping[i].Col2), new(TokenType.InterfaceImpl, mapping[i].Col1)));
            }

            metadata.RemoveInterfaceMapping(type);

            return interfaces;
        }

        private void InitializeInterfaces()
        {
            if (metadata.Interfaces != null)
                return;

            int length = MoveTo(Table.InterfaceImpl);

            metadata.Interfaces = new(length);

            for (uint i = 1; i <= length; i++)
            {
                uint type = ReadTableIndex(Table.TypeDef);
                MetadataToken @interface = ReadMetadataToken(CodedIndex.TypeDefOrRef);

                AddInterfaceMapping(type, new(i, @interface));
            }
        }

        private void AddInterfaceMapping(uint type, Row<uint, MetadataToken> @interface)
        {
            metadata.SetInterfaceMapping(type, AddMapping(metadata.Interfaces, type, @interface));
        }

        public Collection<FieldDefinition> ReadFields(TypeDefinition type)
        {
            Range fields_range = type.fields_range;
            if (fields_range.Length == 0)
                return new MemberDefinitionCollection<FieldDefinition>(type);

            MemberDefinitionCollection<FieldDefinition> fields = new(type, (int)fields_range.Length);
            context = type;

            if (!MoveTo(Table.FieldPtr, fields_range.Start))
            {
                if (!MoveTo(Table.Field, fields_range.Start))
                    return fields;

                for (uint i = 0; i < fields_range.Length; i++)
                    ReadField(fields_range.Start + i, fields);
            }
            else
            {
                ReadPointers(Table.FieldPtr, Table.Field, fields_range, fields, ReadField);
            }

            return fields;
        }

        private void ReadField(uint field_rid, Collection<FieldDefinition> fields)
        {
            FieldAttributes attributes = (FieldAttributes)ReadUInt16();
            string name = ReadString();
            uint signature = ReadBlobIndex();

            FieldDefinition field = new(name, attributes, ReadFieldType(signature));
            field.token = new(TokenType.Field, field_rid);
            metadata.AddFieldDefinition(field);

            if (IsDeleted(field))
                return;

            fields.Add(field);

            if (module.IsWindowsMetadata())
                WindowsRuntimeProjections.Project(field);
        }

        private void InitializeFields()
        {
            if (metadata.Fields != null)
                return;

            metadata.Fields = new FieldDefinition [image.GetTableLength(Table.Field)];
        }

        private TypeReference ReadFieldType(uint signature)
        {
            SignatureReader reader = ReadSignature(signature);

            const byte field_sig = 0x6;

            if (reader.ReadByte() != field_sig)
                throw new NotSupportedException();

            return reader.ReadTypeSignature();
        }

        public int ReadFieldRVA(FieldDefinition field)
        {
            InitializeFieldRVAs();
            uint rid = field.token.RID;

            RVA rva;
            if (!metadata.FieldRVAs.TryGetValue(rid, out rva))
                return 0;

            int size = GetFieldTypeSize(field.FieldType);

            if (size == 0 || rva == 0)
                return 0;

            metadata.FieldRVAs.Remove(rid);

            field.InitialValue = GetFieldInitializeValue(size, rva);

            return (int)rva;
        }

        private byte[] GetFieldInitializeValue(int size, RVA rva)
        {
            return image.GetReaderAt(rva, size, (s, reader) => reader.ReadBytes(s)) ?? Empty<byte>.Array;
        }

        private static int GetFieldTypeSize(TypeReference type)
        {
            int size = 0;

            switch (type.etype)
            {
                case ElementType.Boolean:
                case ElementType.U1:
                case ElementType.I1:
                    size = 1;
                    break;
                case ElementType.U2:
                case ElementType.I2:
                case ElementType.Char:
                    size = 2;
                    break;
                case ElementType.U4:
                case ElementType.I4:
                case ElementType.R4:
                    size = 4;
                    break;
                case ElementType.U8:
                case ElementType.I8:
                case ElementType.R8:
                    size = 8;
                    break;
                case ElementType.Ptr:
                case ElementType.FnPtr:
                    size = IntPtr.Size;
                    break;
                case ElementType.CModOpt:
                case ElementType.CModReqD:
                    return GetFieldTypeSize(((IModifierType)type).ElementType);
                default:
                    TypeDefinition field_type = type.Resolve();
                    if (field_type != null && field_type.HasLayoutInfo)
                        size = field_type.ClassSize;

                    break;
            }

            return size;
        }

        private void InitializeFieldRVAs()
        {
            if (metadata.FieldRVAs != null)
                return;

            int length = MoveTo(Table.FieldRVA);

            Dictionary<uint, uint> field_rvas = metadata.FieldRVAs = new(length);

            for (int i = 0; i < length; i++)
            {
                uint rva = ReadUInt32();
                uint field = ReadTableIndex(Table.Field);

                field_rvas.Add(field, rva);
            }
        }

        public int ReadFieldLayout(FieldDefinition field)
        {
            InitializeFieldLayouts();
            uint rid = field.token.RID;
            uint offset;
            if (!metadata.FieldLayouts.TryGetValue(rid, out offset))
                return Mixin.NoDataMarker;

            metadata.FieldLayouts.Remove(rid);

            return (int)offset;
        }

        private void InitializeFieldLayouts()
        {
            if (metadata.FieldLayouts != null)
                return;

            int length = MoveTo(Table.FieldLayout);

            Dictionary<uint, uint> field_layouts = metadata.FieldLayouts = new(length);

            for (int i = 0; i < length; i++)
            {
                uint offset = ReadUInt32();
                uint field = ReadTableIndex(Table.Field);

                field_layouts.Add(field, offset);
            }
        }

        public bool HasEvents(TypeDefinition type)
        {
            InitializeEvents();

            Range range;
            if (!metadata.TryGetEventsRange(type, out range))
                return false;

            return range.Length > 0;
        }

        public Collection<EventDefinition> ReadEvents(TypeDefinition type)
        {
            InitializeEvents();
            Range range;

            if (!metadata.TryGetEventsRange(type, out range))
                return new MemberDefinitionCollection<EventDefinition>(type);

            MemberDefinitionCollection<EventDefinition> events = new(type, (int)range.Length);

            metadata.RemoveEventsRange(type);

            if (range.Length == 0)
                return events;

            context = type;

            if (!MoveTo(Table.EventPtr, range.Start))
            {
                if (!MoveTo(Table.Event, range.Start))
                    return events;

                for (uint i = 0; i < range.Length; i++)
                    ReadEvent(range.Start + i, events);
            }
            else
            {
                ReadPointers(Table.EventPtr, Table.Event, range, events, ReadEvent);
            }

            return events;
        }

        private void ReadEvent(uint event_rid, Collection<EventDefinition> events)
        {
            EventAttributes attributes = (EventAttributes)ReadUInt16();
            string name = ReadString();
            TypeReference event_type = GetTypeDefOrRef(ReadMetadataToken(CodedIndex.TypeDefOrRef));

            EventDefinition @event = new(name, attributes, event_type);
            @event.token = new(TokenType.Event, event_rid);

            if (IsDeleted(@event))
                return;

            events.Add(@event);
        }

        private void InitializeEvents()
        {
            if (metadata.Events != null)
                return;

            int length = MoveTo(Table.EventMap);

            metadata.Events = new(length);

            for (uint i = 1; i <= length; i++)
            {
                uint type_rid = ReadTableIndex(Table.TypeDef);
                Range events_range = ReadListRange(i, Table.EventMap, Table.Event);
                metadata.AddEventsRange(type_rid, events_range);
            }
        }

        public bool HasProperties(TypeDefinition type)
        {
            InitializeProperties();

            Range range;
            if (!metadata.TryGetPropertiesRange(type, out range))
                return false;

            return range.Length > 0;
        }

        public Collection<PropertyDefinition> ReadProperties(TypeDefinition type)
        {
            InitializeProperties();

            Range range;

            if (!metadata.TryGetPropertiesRange(type, out range))
                return new MemberDefinitionCollection<PropertyDefinition>(type);

            metadata.RemovePropertiesRange(type);

            MemberDefinitionCollection<PropertyDefinition> properties = new(type, (int)range.Length);

            if (range.Length == 0)
                return properties;

            context = type;

            if (!MoveTo(Table.PropertyPtr, range.Start))
            {
                if (!MoveTo(Table.Property, range.Start))
                    return properties;
                for (uint i = 0; i < range.Length; i++)
                    ReadProperty(range.Start + i, properties);
            }
            else
            {
                ReadPointers(Table.PropertyPtr, Table.Property, range, properties, ReadProperty);
            }

            return properties;
        }

        private void ReadProperty(uint property_rid, Collection<PropertyDefinition> properties)
        {
            PropertyAttributes attributes = (PropertyAttributes)ReadUInt16();
            string name = ReadString();
            uint signature = ReadBlobIndex();

            SignatureReader reader = ReadSignature(signature);
            const byte property_signature = 0x8;

            byte calling_convention = reader.ReadByte();

            if ((calling_convention & property_signature) == 0)
                throw new NotSupportedException();

            bool has_this = (calling_convention & 0x20) != 0;

            reader.ReadCompressedUInt32(); // count

            PropertyDefinition property = new(name, attributes, reader.ReadTypeSignature());
            property.HasThis = has_this;
            property.token = new(TokenType.Property, property_rid);

            if (IsDeleted(property))
                return;

            properties.Add(property);
        }

        private void InitializeProperties()
        {
            if (metadata.Properties != null)
                return;

            int length = MoveTo(Table.PropertyMap);

            metadata.Properties = new(length);

            for (uint i = 1; i <= length; i++)
            {
                uint type_rid = ReadTableIndex(Table.TypeDef);
                Range properties_range = ReadListRange(i, Table.PropertyMap, Table.Property);
                metadata.AddPropertiesRange(type_rid, properties_range);
            }
        }

        private MethodSemanticsAttributes ReadMethodSemantics(MethodDefinition method)
        {
            InitializeMethodSemantics();
            Row<MethodSemanticsAttributes, MetadataToken> row;
            if (!metadata.Semantics.TryGetValue(method.token.RID, out row))
                return MethodSemanticsAttributes.None;

            TypeDefinition type = method.DeclaringType;

            switch (row.Col1)
            {
                case MethodSemanticsAttributes.AddOn:
                    GetEvent(type, row.Col2).add_method = method;
                    break;
                case MethodSemanticsAttributes.Fire:
                    GetEvent(type, row.Col2).invoke_method = method;
                    break;
                case MethodSemanticsAttributes.RemoveOn:
                    GetEvent(type, row.Col2).remove_method = method;
                    break;
                case MethodSemanticsAttributes.Getter:
                    GetProperty(type, row.Col2).get_method = method;
                    break;
                case MethodSemanticsAttributes.Setter:
                    GetProperty(type, row.Col2).set_method = method;
                    break;
                case MethodSemanticsAttributes.Other:
                    switch (row.Col2.TokenType)
                    {
                        case TokenType.Event:
                        {
                            EventDefinition @event = GetEvent(type, row.Col2);
                            if (@event.other_methods == null)
                                @event.other_methods = new();

                            @event.other_methods.Add(method);
                            break;
                        }
                        case TokenType.Property:
                        {
                            PropertyDefinition property = GetProperty(type, row.Col2);
                            if (property.other_methods == null)
                                property.other_methods = new();

                            property.other_methods.Add(method);

                            break;
                        }
                        default:
                            throw new NotSupportedException();
                    }
                    break;
                default:
                    throw new NotSupportedException();
            }

            metadata.Semantics.Remove(method.token.RID);

            return row.Col1;
        }

        private static EventDefinition GetEvent(TypeDefinition type, MetadataToken token)
        {
            if (token.TokenType != TokenType.Event)
                throw new ArgumentException();

            return GetMember(type.Events, token);
        }

        private static PropertyDefinition GetProperty(TypeDefinition type, MetadataToken token)
        {
            if (token.TokenType != TokenType.Property)
                throw new ArgumentException();

            return GetMember(type.Properties, token);
        }

        private static TMember GetMember<TMember>(Collection<TMember> members, MetadataToken token) where TMember : IMemberDefinition
        {
            for (int i = 0; i < members.Count; i++)
            {
                TMember member = members[i];
                if (member.MetadataToken == token)
                    return member;
            }

            throw new ArgumentException();
        }

        private void InitializeMethodSemantics()
        {
            if (metadata.Semantics != null)
                return;

            int length = MoveTo(Table.MethodSemantics);

            Dictionary<uint, Row<MethodSemanticsAttributes, MetadataToken>> semantics = metadata.Semantics = new(0);

            for (uint i = 0; i < length; i++)
            {
                MethodSemanticsAttributes attributes = (MethodSemanticsAttributes)ReadUInt16();
                uint method_rid = ReadTableIndex(Table.Method);
                MetadataToken association = ReadMetadataToken(CodedIndex.HasSemantics);

                semantics[method_rid] = new(attributes, association);
            }
        }

        public void ReadMethods(PropertyDefinition property)
        {
            ReadAllSemantics(property.DeclaringType);
        }

        public void ReadMethods(EventDefinition @event)
        {
            ReadAllSemantics(@event.DeclaringType);
        }

        public void ReadAllSemantics(MethodDefinition method)
        {
            ReadAllSemantics(method.DeclaringType);
        }

        private void ReadAllSemantics(TypeDefinition type)
        {
            Collection<MethodDefinition> methods = type.Methods;
            for (int i = 0; i < methods.Count; i++)
            {
                MethodDefinition method = methods[i];
                if (method.sem_attrs_ready)
                    continue;

                method.sem_attrs = ReadMethodSemantics(method);
                method.sem_attrs_ready = true;
            }
        }

        public Collection<MethodDefinition> ReadMethods(TypeDefinition type)
        {
            Range methods_range = type.methods_range;
            if (methods_range.Length == 0)
                return new MemberDefinitionCollection<MethodDefinition>(type);

            MemberDefinitionCollection<MethodDefinition> methods = new(type, (int)methods_range.Length);
            if (!MoveTo(Table.MethodPtr, methods_range.Start))
            {
                if (!MoveTo(Table.Method, methods_range.Start))
                    return methods;

                for (uint i = 0; i < methods_range.Length; i++)
                    ReadMethod(methods_range.Start + i, methods);
            }
            else
            {
                ReadPointers(Table.MethodPtr, Table.Method, methods_range, methods, ReadMethod);
            }

            return methods;
        }

        private void ReadPointers<TMember>(Table ptr, Table table, Range range, Collection<TMember> members, Action<uint, Collection<TMember>> reader) where TMember : IMemberDefinition
        {
            for (uint i = 0; i < range.Length; i++)
            {
                MoveTo(ptr, range.Start + i);

                uint rid = ReadTableIndex(table);
                MoveTo(table, rid);

                reader(rid, members);
            }
        }

        private static bool IsDeleted(IMemberDefinition member)
        {
            return member.IsSpecialName && member.Name == "_Deleted";
        }

        private void InitializeMethods()
        {
            if (metadata.Methods != null)
                return;

            metadata.Methods = new MethodDefinition [image.GetTableLength(Table.Method)];
        }

        private void ReadMethod(uint method_rid, Collection<MethodDefinition> methods)
        {
            MethodDefinition method = new();
            method.rva = ReadUInt32();
            method.ImplAttributes = (MethodImplAttributes)ReadUInt16();
            method.Attributes = (MethodAttributes)ReadUInt16();
            method.Name = ReadString();
            method.token = new(TokenType.Method, method_rid);

            if (IsDeleted(method))
                return;

            methods.Add(method); // attach method

            uint signature = ReadBlobIndex();
            Range param_range = ReadListRange(method_rid, Table.Method, Table.Param);

            context = method;

            ReadMethodSignature(signature, method);
            metadata.AddMethodDefinition(method);

            if (param_range.Length != 0)
            {
                int position = this.position;
                ReadParameters(method, param_range);
                this.position = position;
            }

            if (module.IsWindowsMetadata())
                WindowsRuntimeProjections.Project(method);
        }

        private void ReadParameters(MethodDefinition method, Range param_range)
        {
            if (!MoveTo(Table.ParamPtr, param_range.Start))
            {
                if (!MoveTo(Table.Param, param_range.Start))
                    return;

                for (uint i = 0; i < param_range.Length; i++)
                    ReadParameter(param_range.Start + i, method);
            }
            else
            {
                ReadParameterPointers(method, param_range);
            }
        }

        private void ReadParameterPointers(MethodDefinition method, Range range)
        {
            for (uint i = 0; i < range.Length; i++)
            {
                MoveTo(Table.ParamPtr, range.Start + i);

                uint rid = ReadTableIndex(Table.Param);

                MoveTo(Table.Param, rid);

                ReadParameter(rid, method);
            }
        }

        private void ReadParameter(uint param_rid, MethodDefinition method)
        {
            ParameterAttributes attributes = (ParameterAttributes)ReadUInt16();
            ushort sequence = ReadUInt16();
            string name = ReadString();

            ParameterDefinition parameter = sequence == 0 ? method.MethodReturnType.Parameter : method.Parameters[sequence - 1];

            parameter.token = new(TokenType.Param, param_rid);
            parameter.Name = name;
            parameter.Attributes = attributes;
        }

        private void ReadMethodSignature(uint signature, IMethodSignature method)
        {
            SignatureReader reader = ReadSignature(signature);
            reader.ReadMethodSignature(method);
        }

        public PInvokeInfo ReadPInvokeInfo(MethodDefinition method)
        {
            InitializePInvokes();
            Row<PInvokeAttributes, uint, uint> row;

            uint rid = method.token.RID;

            if (!metadata.PInvokes.TryGetValue(rid, out row))
                return null;

            metadata.PInvokes.Remove(rid);

            return new(row.Col1, image.StringHeap.Read(row.Col2), module.ModuleReferences[(int)row.Col3 - 1]);
        }

        private void InitializePInvokes()
        {
            if (metadata.PInvokes != null)
                return;

            int length = MoveTo(Table.ImplMap);

            Dictionary<uint, Row<PInvokeAttributes, uint, uint>> pinvokes = metadata.PInvokes = new(length);

            for (int i = 1; i <= length; i++)
            {
                PInvokeAttributes attributes = (PInvokeAttributes)ReadUInt16();
                MetadataToken method = ReadMetadataToken(CodedIndex.MemberForwarded);
                uint name = ReadStringIndex();
                uint scope = ReadTableIndex(Table.File);

                if (method.TokenType != TokenType.Method)
                    continue;

                pinvokes.Add(method.RID, new(attributes, name, scope));
            }
        }

        public bool HasGenericParameters(IGenericParameterProvider provider)
        {
            InitializeGenericParameters();

            Range[] ranges;
            if (!metadata.TryGetGenericParameterRanges(provider, out ranges))
                return false;

            return RangesSize(ranges) > 0;
        }

        public Collection<GenericParameter> ReadGenericParameters(IGenericParameterProvider provider)
        {
            InitializeGenericParameters();

            Range[] ranges;
            if (!metadata.TryGetGenericParameterRanges(provider, out ranges))
                return new GenericParameterCollection(provider);

            metadata.RemoveGenericParameterRange(provider);

            GenericParameterCollection generic_parameters = new(provider, RangesSize(ranges));

            for (int i = 0; i < ranges.Length; i++)
                ReadGenericParametersRange(ranges[i], provider, generic_parameters);

            return generic_parameters;
        }

        private void ReadGenericParametersRange(Range range, IGenericParameterProvider provider, GenericParameterCollection generic_parameters)
        {
            if (!MoveTo(Table.GenericParam, range.Start))
                return;

            for (uint i = 0; i < range.Length; i++)
            {
                ReadUInt16(); // index
                GenericParameterAttributes flags = (GenericParameterAttributes)ReadUInt16();
                ReadMetadataToken(CodedIndex.TypeOrMethodDef);
                string name = ReadString();

                GenericParameter parameter = new(name, provider);
                parameter.token = new(TokenType.GenericParam, range.Start + i);
                parameter.Attributes = flags;

                generic_parameters.Add(parameter);
            }
        }

        private void InitializeGenericParameters()
        {
            if (metadata.GenericParameters != null)
                return;

            metadata.GenericParameters = InitializeRanges(Table.GenericParam, () =>
            {
                Advance(4);
                MetadataToken next = ReadMetadataToken(CodedIndex.TypeOrMethodDef);
                ReadStringIndex();
                return next;
            });
        }

        private Dictionary<MetadataToken, Range[]> InitializeRanges(Table table, Func<MetadataToken> get_next)
        {
            int length = MoveTo(table);
            Dictionary<MetadataToken, Range[]> ranges = new(length);

            if (length == 0)
                return ranges;

            MetadataToken owner = MetadataToken.Zero;
            Range range = new(1, 0);

            for (uint i = 1; i <= length; i++)
            {
                MetadataToken next = get_next();

                if (i == 1)
                {
                    owner = next;
                    range.Length++;
                }
                else if (next != owner)
                {
                    AddRange(ranges, owner, range);
                    range = new(i, 1);
                    owner = next;
                }
                else
                {
                    range.Length++;
                }
            }

            AddRange(ranges, owner, range);

            return ranges;
        }

        private static void AddRange(Dictionary<MetadataToken, Range[]> ranges, MetadataToken owner, Range range)
        {
            if (owner.RID == 0)
                return;

            Range[] slots;
            if (!ranges.TryGetValue(owner, out slots))
            {
                ranges.Add(owner, new[] { range });
                return;
            }

            ranges[owner] = slots.Add(range);
        }

        public bool HasGenericConstraints(GenericParameter generic_parameter)
        {
            InitializeGenericConstraints();

            Collection<Row<uint, MetadataToken>> mapping;
            if (!metadata.TryGetGenericConstraintMapping(generic_parameter, out mapping))
                return false;

            return mapping.Count > 0;
        }

        public GenericParameterConstraintCollection ReadGenericConstraints(GenericParameter generic_parameter)
        {
            InitializeGenericConstraints();

            Collection<Row<uint, MetadataToken>> mapping;
            if (!metadata.TryGetGenericConstraintMapping(generic_parameter, out mapping))
                return new(generic_parameter);

            GenericParameterConstraintCollection constraints = new(generic_parameter, mapping.Count);

            context = (IGenericContext)generic_parameter.Owner;

            for (int i = 0; i < mapping.Count; i++)
            {
                constraints.Add(new(GetTypeDefOrRef(mapping[i].Col2), new(TokenType.GenericParamConstraint, mapping[i].Col1)));
            }

            metadata.RemoveGenericConstraintMapping(generic_parameter);

            return constraints;
        }

        private void InitializeGenericConstraints()
        {
            if (metadata.GenericConstraints != null)
                return;

            int length = MoveTo(Table.GenericParamConstraint);

            metadata.GenericConstraints = new(length);

            for (uint i = 1; i <= length; i++)
            {
                AddGenericConstraintMapping(ReadTableIndex(Table.GenericParam), new(i, ReadMetadataToken(CodedIndex.TypeDefOrRef)));
            }
        }

        private void AddGenericConstraintMapping(uint generic_parameter, Row<uint, MetadataToken> constraint)
        {
            metadata.SetGenericConstraintMapping(generic_parameter, AddMapping(metadata.GenericConstraints, generic_parameter, constraint));
        }

        public bool HasOverrides(MethodDefinition method)
        {
            InitializeOverrides();
            Collection<MetadataToken> mapping;

            if (!metadata.TryGetOverrideMapping(method, out mapping))
                return false;

            return mapping.Count > 0;
        }

        public Collection<MethodReference> ReadOverrides(MethodDefinition method)
        {
            InitializeOverrides();

            Collection<MetadataToken> mapping;
            if (!metadata.TryGetOverrideMapping(method, out mapping))
                return new();

            Collection<MethodReference> overrides = new(mapping.Count);

            context = method;

            for (int i = 0; i < mapping.Count; i++)
                overrides.Add((MethodReference)LookupToken(mapping[i]));

            metadata.RemoveOverrideMapping(method);

            return overrides;
        }

        private void InitializeOverrides()
        {
            if (metadata.Overrides != null)
                return;

            int length = MoveTo(Table.MethodImpl);

            metadata.Overrides = new(length);

            for (int i = 1; i <= length; i++)
            {
                ReadTableIndex(Table.TypeDef);

                MetadataToken method = ReadMetadataToken(CodedIndex.MethodDefOrRef);
                if (method.TokenType != TokenType.Method)
                    throw new NotSupportedException();

                MetadataToken @override = ReadMetadataToken(CodedIndex.MethodDefOrRef);

                AddOverrideMapping(method.RID, @override);
            }
        }

        private void AddOverrideMapping(uint method_rid, MetadataToken @override)
        {
            metadata.SetOverrideMapping(method_rid, AddMapping(metadata.Overrides, method_rid, @override));
        }

        public MethodBody ReadMethodBody(MethodDefinition method)
        {
            return code.ReadMethodBody(method);
        }

        public int ReadCodeSize(MethodDefinition method)
        {
            return code.ReadCodeSize(method);
        }

        public CallSite ReadCallSite(MetadataToken token)
        {
            if (!MoveTo(Table.StandAloneSig, token.RID))
                return null;

            uint signature = ReadBlobIndex();

            CallSite call_site = new();

            ReadMethodSignature(signature, call_site);

            call_site.MetadataToken = token;

            return call_site;
        }

        public VariableDefinitionCollection ReadVariables(MetadataToken local_var_token, MethodDefinition method = null)
        {
            if (!MoveTo(Table.StandAloneSig, local_var_token.RID))
                return null;

            SignatureReader reader = ReadSignature(ReadBlobIndex());
            const byte local_sig = 0x7;

            if (reader.ReadByte() != local_sig)
                throw new NotSupportedException();

            uint count = reader.ReadCompressedUInt32();
            if (count == 0)
                return null;

            VariableDefinitionCollection variables = new(method, (int)count);

            for (int i = 0; i < count; i++)
                variables.Add(new(reader.ReadTypeSignature()));

            return variables;
        }

        public IMetadataTokenProvider LookupToken(MetadataToken token)
        {
            uint rid = token.RID;

            if (rid == 0)
                return null;

            if (metadata_reader != null)
                return metadata_reader.LookupToken(token);

            IMetadataTokenProvider element;
            int position = this.position;
            IGenericContext context = this.context;

            switch (token.TokenType)
            {
                case TokenType.TypeDef:
                    element = GetTypeDefinition(rid);
                    break;
                case TokenType.TypeRef:
                    element = GetTypeReference(rid);
                    break;
                case TokenType.TypeSpec:
                    element = GetTypeSpecification(rid);
                    break;
                case TokenType.Field:
                    element = GetFieldDefinition(rid);
                    break;
                case TokenType.Method:
                    element = GetMethodDefinition(rid);
                    break;
                case TokenType.MemberRef:
                    element = GetMemberReference(rid);
                    break;
                case TokenType.MethodSpec:
                    element = GetMethodSpecification(rid);
                    break;
                default:
                    return null;
            }

            this.position = position;
            this.context = context;

            return element;
        }

        public FieldDefinition GetFieldDefinition(uint rid)
        {
            InitializeTypeDefinitions();

            FieldDefinition field = metadata.GetFieldDefinition(rid);
            if (field != null)
                return field;

            return LookupField(rid);
        }

        private FieldDefinition LookupField(uint rid)
        {
            TypeDefinition type = metadata.GetFieldDeclaringType(rid);
            if (type == null)
                return null;

            Mixin.Read(type.Fields);

            return metadata.GetFieldDefinition(rid);
        }

        public MethodDefinition GetMethodDefinition(uint rid)
        {
            InitializeTypeDefinitions();

            MethodDefinition method = metadata.GetMethodDefinition(rid);
            if (method != null)
                return method;

            return LookupMethod(rid);
        }

        private MethodDefinition LookupMethod(uint rid)
        {
            TypeDefinition type = metadata.GetMethodDeclaringType(rid);
            if (type == null)
                return null;

            Mixin.Read(type.Methods);

            return metadata.GetMethodDefinition(rid);
        }

        private MethodSpecification GetMethodSpecification(uint rid)
        {
            if (!MoveTo(Table.MethodSpec, rid))
                return null;

            MethodReference element_method = (MethodReference)LookupToken(ReadMetadataToken(CodedIndex.MethodDefOrRef));
            uint signature = ReadBlobIndex();

            MethodSpecification method_spec = ReadMethodSpecSignature(signature, element_method);
            method_spec.token = new(TokenType.MethodSpec, rid);
            return method_spec;
        }

        private MethodSpecification ReadMethodSpecSignature(uint signature, MethodReference method)
        {
            SignatureReader reader = ReadSignature(signature);
            const byte methodspec_sig = 0x0a;

            byte call_conv = reader.ReadByte();

            if (call_conv != methodspec_sig)
                throw new NotSupportedException();

            uint arity = reader.ReadCompressedUInt32();

            GenericInstanceMethod instance = new(method, (int)arity);

            reader.ReadGenericInstanceSignature(method, instance, arity);

            return instance;
        }

        private MemberReference GetMemberReference(uint rid)
        {
            InitializeMemberReferences();

            MemberReference member = metadata.GetMemberReference(rid);
            if (member != null)
                return member;

            member = ReadMemberReference(rid);
            if (member != null && !member.ContainsGenericParameter)
                metadata.AddMemberReference(member);
            return member;
        }

        private MemberReference ReadMemberReference(uint rid)
        {
            if (!MoveTo(Table.MemberRef, rid))
                return null;

            MetadataToken token = ReadMetadataToken(CodedIndex.MemberRefParent);
            string name = ReadString();
            uint signature = ReadBlobIndex();

            MemberReference member;

            switch (token.TokenType)
            {
                case TokenType.TypeDef:
                case TokenType.TypeRef:
                case TokenType.TypeSpec:
                    member = ReadTypeMemberReference(token, name, signature);
                    break;
                case TokenType.Method:
                    member = ReadMethodMemberReference(token, name, signature);
                    break;
                default:
                    throw new NotSupportedException();
            }

            member.token = new(TokenType.MemberRef, rid);
            return member;
        }

        private MemberReference ReadTypeMemberReference(MetadataToken type, string name, uint signature)
        {
            TypeReference declaring_type = GetTypeDefOrRef(type);

            if (!declaring_type.IsArray)
                context = declaring_type;

            MemberReference member = ReadMemberReferenceSignature(signature, declaring_type);
            member.Name = name;

            return member;
        }

        private MemberReference ReadMemberReferenceSignature(uint signature, TypeReference declaring_type)
        {
            SignatureReader reader = ReadSignature(signature);
            const byte field_sig = 0x6;

            if (reader.buffer[reader.position] == field_sig)
            {
                reader.position++;
                FieldReference field = new();
                field.DeclaringType = declaring_type;
                field.FieldType = reader.ReadTypeSignature();
                return field;
            }
            else
            {
                MethodReference method = new();
                method.DeclaringType = declaring_type;
                reader.ReadMethodSignature(method);
                return method;
            }
        }

        private MemberReference ReadMethodMemberReference(MetadataToken token, string name, uint signature)
        {
            MethodDefinition method = GetMethodDefinition(token.RID);

            context = method;

            MemberReference member = ReadMemberReferenceSignature(signature, method.DeclaringType);
            member.Name = name;

            return member;
        }

        private void InitializeMemberReferences()
        {
            if (metadata.MemberReferences != null)
                return;

            metadata.MemberReferences = new MemberReference [image.GetTableLength(Table.MemberRef)];
        }

        public IEnumerable<MemberReference> GetMemberReferences()
        {
            InitializeMemberReferences();

            int length = image.GetTableLength(Table.MemberRef);

            TypeSystem type_system = module.TypeSystem;

            MethodDefinition context = new(string.Empty, MethodAttributes.Static, type_system.Void);
            context.DeclaringType = new(string.Empty, string.Empty, TypeAttributes.Public);

            MemberReference[] member_references = new MemberReference [length];

            for (uint i = 1; i <= length; i++)
            {
                this.context = context;
                member_references[i - 1] = GetMemberReference(i);
            }

            return member_references;
        }

        private void InitializeConstants()
        {
            if (metadata.Constants != null)
                return;

            int length = MoveTo(Table.Constant);

            Dictionary<MetadataToken, Row<ElementType, uint>> constants = metadata.Constants = new(length);

            for (uint i = 1; i <= length; i++)
            {
                ElementType type = (ElementType)ReadUInt16();
                MetadataToken owner = ReadMetadataToken(CodedIndex.HasConstant);
                uint signature = ReadBlobIndex();

                constants.Add(owner, new(type, signature));
            }
        }

        public TypeReference ReadConstantSignature(MetadataToken token)
        {
            if (token.TokenType != TokenType.Signature)
                throw new NotSupportedException();

            if (token.RID == 0)
                return null;

            if (!MoveTo(Table.StandAloneSig, token.RID))
                return null;

            return ReadFieldType(ReadBlobIndex());
        }

        public object ReadConstant(IConstantProvider owner)
        {
            InitializeConstants();

            Row<ElementType, uint> row;
            if (!metadata.Constants.TryGetValue(owner.MetadataToken, out row))
                return Mixin.NoValue;

            metadata.Constants.Remove(owner.MetadataToken);

            return ReadConstantValue(row.Col1, row.Col2);
        }

        private object ReadConstantValue(ElementType etype, uint signature)
        {
            switch (etype)
            {
                case ElementType.Class:
                case ElementType.Object:
                    return null;
                case ElementType.String:
                    return ReadConstantString(signature);
                default:
                    return ReadConstantPrimitive(etype, signature);
            }
        }

        private string ReadConstantString(uint signature)
        {
            byte[] blob;
            int index, count;

            GetBlobView(signature, out blob, out index, out count);
            if (count == 0)
                return string.Empty;

            if ((count & 1) == 1)
                count--;

            return Encoding.Unicode.GetString(blob, index, count);
        }

        private object ReadConstantPrimitive(ElementType type, uint signature)
        {
            SignatureReader reader = ReadSignature(signature);
            return reader.ReadConstantSignature(type);
        }

        internal void InitializeCustomAttributes()
        {
            if (metadata.CustomAttributes != null)
                return;

            metadata.CustomAttributes = InitializeRanges(Table.CustomAttribute, () =>
            {
                MetadataToken next = ReadMetadataToken(CodedIndex.HasCustomAttribute);
                ReadMetadataToken(CodedIndex.CustomAttributeType);
                ReadBlobIndex();
                return next;
            });
        }

        public bool HasCustomAttributes(ICustomAttributeProvider owner)
        {
            InitializeCustomAttributes();

            Range[] ranges;
            if (!metadata.TryGetCustomAttributeRanges(owner, out ranges))
                return false;

            return RangesSize(ranges) > 0;
        }

        public Collection<CustomAttribute> ReadCustomAttributes(ICustomAttributeProvider owner)
        {
            InitializeCustomAttributes();

            Range[] ranges;
            if (!metadata.TryGetCustomAttributeRanges(owner, out ranges))
                return new();

            Collection<CustomAttribute> custom_attributes = new(RangesSize(ranges));

            for (int i = 0; i < ranges.Length; i++)
                ReadCustomAttributeRange(ranges[i], custom_attributes);

            metadata.RemoveCustomAttributeRange(owner);

            if (module.IsWindowsMetadata())
                foreach (CustomAttribute custom_attribute in custom_attributes)
                    WindowsRuntimeProjections.Project(owner, custom_attribute);

            return custom_attributes;
        }

        private void ReadCustomAttributeRange(Range range, Collection<CustomAttribute> custom_attributes)
        {
            if (!MoveTo(Table.CustomAttribute, range.Start))
                return;

            for (int i = 0; i < range.Length; i++)
            {
                ReadMetadataToken(CodedIndex.HasCustomAttribute);

                MethodReference constructor = (MethodReference)LookupToken(ReadMetadataToken(CodedIndex.CustomAttributeType));

                uint signature = ReadBlobIndex();

                custom_attributes.Add(new(signature, constructor));
            }
        }

        private static int RangesSize(Range[] ranges)
        {
            uint size = 0;
            for (int i = 0; i < ranges.Length; i++)
                size += ranges[i].Length;

            return (int)size;
        }

        public IEnumerable<CustomAttribute> GetCustomAttributes()
        {
            InitializeTypeDefinitions();

            uint length = image.TableHeap[Table.CustomAttribute].Length;
            Collection<CustomAttribute> custom_attributes = new((int)length);
            ReadCustomAttributeRange(new(1, length), custom_attributes);

            return custom_attributes;
        }

        public byte[] ReadCustomAttributeBlob(uint signature)
        {
            return ReadBlob(signature);
        }

        public void ReadCustomAttributeSignature(CustomAttribute attribute)
        {
            SignatureReader reader = ReadSignature(attribute.signature);

            if (!reader.CanReadMore())
                return;

            if (reader.ReadUInt16() != 0x0001)
                throw new InvalidOperationException();

            MethodReference constructor = attribute.Constructor;
            if (constructor.HasParameters)
                reader.ReadCustomAttributeConstructorArguments(attribute, constructor.Parameters);

            if (!reader.CanReadMore())
                return;

            ushort named = reader.ReadUInt16();

            if (named == 0)
                return;

            reader.ReadCustomAttributeNamedArguments(named, ref attribute.fields, ref attribute.properties);
        }

        private void InitializeMarshalInfos()
        {
            if (metadata.FieldMarshals != null)
                return;

            int length = MoveTo(Table.FieldMarshal);

            Dictionary<MetadataToken, uint> marshals = metadata.FieldMarshals = new(length);

            for (int i = 0; i < length; i++)
            {
                MetadataToken token = ReadMetadataToken(CodedIndex.HasFieldMarshal);
                uint signature = ReadBlobIndex();
                if (token.RID == 0)
                    continue;

                marshals.Add(token, signature);
            }
        }

        public bool HasMarshalInfo(IMarshalInfoProvider owner)
        {
            InitializeMarshalInfos();

            return metadata.FieldMarshals.ContainsKey(owner.MetadataToken);
        }

        public MarshalInfo ReadMarshalInfo(IMarshalInfoProvider owner)
        {
            InitializeMarshalInfos();

            uint signature;
            if (!metadata.FieldMarshals.TryGetValue(owner.MetadataToken, out signature))
                return null;

            SignatureReader reader = ReadSignature(signature);

            metadata.FieldMarshals.Remove(owner.MetadataToken);

            return reader.ReadMarshalInfo();
        }

        private void InitializeSecurityDeclarations()
        {
            if (metadata.SecurityDeclarations != null)
                return;

            metadata.SecurityDeclarations = InitializeRanges(Table.DeclSecurity, () =>
            {
                ReadUInt16();
                MetadataToken next = ReadMetadataToken(CodedIndex.HasDeclSecurity);
                ReadBlobIndex();
                return next;
            });
        }

        public bool HasSecurityDeclarations(ISecurityDeclarationProvider owner)
        {
            InitializeSecurityDeclarations();

            Range[] ranges;
            if (!metadata.TryGetSecurityDeclarationRanges(owner, out ranges))
                return false;

            return RangesSize(ranges) > 0;
        }

        public Collection<SecurityDeclaration> ReadSecurityDeclarations(ISecurityDeclarationProvider owner)
        {
            InitializeSecurityDeclarations();

            Range[] ranges;
            if (!metadata.TryGetSecurityDeclarationRanges(owner, out ranges))
                return new();

            Collection<SecurityDeclaration> security_declarations = new(RangesSize(ranges));

            for (int i = 0; i < ranges.Length; i++)
                ReadSecurityDeclarationRange(ranges[i], security_declarations);

            metadata.RemoveSecurityDeclarationRange(owner);

            return security_declarations;
        }

        private void ReadSecurityDeclarationRange(Range range, Collection<SecurityDeclaration> security_declarations)
        {
            if (!MoveTo(Table.DeclSecurity, range.Start))
                return;

            for (int i = 0; i < range.Length; i++)
            {
                SecurityAction action = (SecurityAction)ReadUInt16();
                ReadMetadataToken(CodedIndex.HasDeclSecurity);
                uint signature = ReadBlobIndex();

                security_declarations.Add(new(action, signature, module));
            }
        }

        public byte[] ReadSecurityDeclarationBlob(uint signature)
        {
            return ReadBlob(signature);
        }

        public void ReadSecurityDeclarationSignature(SecurityDeclaration declaration)
        {
            uint signature = declaration.signature;
            SignatureReader reader = ReadSignature(signature);

            if (reader.buffer[reader.position] != '.')
            {
                ReadXmlSecurityDeclaration(signature, declaration);
                return;
            }

            reader.position++;
            uint count = reader.ReadCompressedUInt32();
            Collection<SecurityAttribute> attributes = new((int)count);

            for (int i = 0; i < count; i++)
                attributes.Add(reader.ReadSecurityAttribute());

            declaration.security_attributes = attributes;
        }

        private void ReadXmlSecurityDeclaration(uint signature, SecurityDeclaration declaration)
        {
            Collection<SecurityAttribute> attributes = new(1);

            SecurityAttribute attribute = new(module.TypeSystem.LookupType("System.Security.Permissions", "PermissionSetAttribute"));

            attribute.properties = new(1);
            attribute.properties.Add(new("XML", new(module.TypeSystem.String, ReadUnicodeStringBlob(signature))));

            attributes.Add(attribute);

            declaration.security_attributes = attributes;
        }

        public Collection<ExportedType> ReadExportedTypes()
        {
            int length = MoveTo(Table.ExportedType);
            if (length == 0)
                return new();

            Collection<ExportedType> exported_types = new(length);

            for (int i = 1; i <= length; i++)
            {
                TypeAttributes attributes = (TypeAttributes)ReadUInt32();
                uint identifier = ReadUInt32();
                string name = ReadString();
                string @namespace = ReadString();
                MetadataToken implementation = ReadMetadataToken(CodedIndex.Implementation);

                ExportedType declaring_type = null;
                IMetadataScope scope = null;

                switch (implementation.TokenType)
                {
                    case TokenType.AssemblyRef:
                    case TokenType.File:
                        scope = GetExportedTypeScope(implementation);
                        break;
                    case TokenType.ExportedType:
                        // FIXME: if the table is not properly sorted
                        declaring_type = exported_types[(int)implementation.RID - 1];
                        break;
                }

                ExportedType exported_type = new(@namespace, name, module, scope)
                {
                    Attributes = attributes,
                    Identifier = (int)identifier,
                    DeclaringType = declaring_type
                };
                exported_type.token = new(TokenType.ExportedType, i);

                exported_types.Add(exported_type);
            }

            return exported_types;
        }

        private IMetadataScope GetExportedTypeScope(MetadataToken token)
        {
            int position = this.position;
            IMetadataScope scope;

            switch (token.TokenType)
            {
                case TokenType.AssemblyRef:
                    InitializeAssemblyReferences();
                    scope = metadata.GetAssemblyNameReference(token.RID);
                    break;
                case TokenType.File:
                    InitializeModuleReferences();
                    scope = GetModuleReferenceFromFile(token);
                    break;
                default:
                    throw new NotSupportedException();
            }

            this.position = position;
            return scope;
        }

        private ModuleReference GetModuleReferenceFromFile(MetadataToken token)
        {
            if (!MoveTo(Table.File, token.RID))
                return null;

            ReadUInt32();
            string file_name = ReadString();
            Collection<ModuleReference> modules = module.ModuleReferences;

            ModuleReference reference;
            for (int i = 0; i < modules.Count; i++)
            {
                reference = modules[i];
                if (reference.Name == file_name)
                    return reference;
            }

            reference = new(file_name);
            modules.Add(reference);
            return reference;
        }

        private void InitializeDocuments()
        {
            if (metadata.Documents != null)
                return;

            int length = MoveTo(Table.Document);

            Document[] documents = metadata.Documents = new Document [length];

            for (uint i = 1; i <= length; i++)
            {
                uint name_index = ReadBlobIndex();
                Guid hash_algorithm = ReadGuid();
                byte[] hash = ReadBlob();
                Guid language = ReadGuid();

                SignatureReader signature = ReadSignature(name_index);
                string name = signature.ReadDocumentName();

                documents[i - 1] = new(name)
                {
                    HashAlgorithmGuid = hash_algorithm,
                    Hash = hash,
                    LanguageGuid = language,
                    token = new(TokenType.Document, i)
                };
            }
        }

        public Collection<SequencePoint> ReadSequencePoints(MethodDefinition method)
        {
            InitializeDocuments();

            if (!MoveTo(Table.MethodDebugInformation, method.MetadataToken.RID))
                return new(0);

            uint document_index = ReadTableIndex(Table.Document);
            uint signature = ReadBlobIndex();
            if (signature == 0)
                return new(0);

            Document document = GetDocument(document_index);
            SignatureReader reader = ReadSignature(signature);

            return reader.ReadSequencePoints(document);
        }

        public Document GetDocument(uint rid)
        {
            Document document = metadata.GetDocument(rid);
            if (document == null)
                return null;

            document.custom_infos = GetCustomDebugInformation(document);
            return document;
        }

        private void InitializeLocalScopes()
        {
            if (metadata.LocalScopes != null)
                return;

            InitializeMethods();

            int length = MoveTo(Table.LocalScope);

            metadata.LocalScopes = new();

            for (uint i = 1; i <= length; i++)
            {
                uint method = ReadTableIndex(Table.Method);
                uint import = ReadTableIndex(Table.ImportScope);
                Range variables = ReadListRange(i, Table.LocalScope, Table.LocalVariable);
                Range constants = ReadListRange(i, Table.LocalScope, Table.LocalConstant);
                uint scope_start = ReadUInt32();
                uint scope_length = ReadUInt32();

                metadata.SetLocalScopes(method, AddMapping(metadata.LocalScopes, method, new(import, variables, constants, scope_start, scope_length, i)));
            }
        }

        public ScopeDebugInformation ReadScope(MethodDefinition method)
        {
            InitializeLocalScopes();
            InitializeImportScopes();

            Collection<Row<uint, Range, Range, uint, uint, uint>> records;
            if (!metadata.TryGetLocalScopes(method, out records))
                return null;

            ScopeDebugInformation method_scope = null as ScopeDebugInformation;

            for (int i = 0; i < records.Count; i++)
            {
                ScopeDebugInformation scope = ReadLocalScope(records[i]);

                if (i == 0)
                {
                    method_scope = scope;
                    continue;
                }

                if (!AddScope(method_scope.scopes, scope))
                    method_scope.Scopes.Add(scope);
            }

            return method_scope;
        }

        private static bool AddScope(Collection<ScopeDebugInformation> scopes, ScopeDebugInformation scope)
        {
            if (scopes.IsNullOrEmpty())
                return false;

            foreach (ScopeDebugInformation sub_scope in scopes)
            {
                if (sub_scope.HasScopes && AddScope(sub_scope.Scopes, scope))
                    return true;

                if (scope.Start.Offset >= sub_scope.Start.Offset && scope.End.Offset <= sub_scope.End.Offset)
                {
                    sub_scope.Scopes.Add(scope);
                    return true;
                }
            }

            return false;
        }

        private ScopeDebugInformation ReadLocalScope(Row<uint, Range, Range, uint, uint, uint> record)
        {
            ScopeDebugInformation scope = new()
            {
                start = new((int)record.Col4),
                end = new((int)(record.Col4 + record.Col5)),
                token = new(TokenType.LocalScope, record.Col6)
            };

            if (record.Col1 > 0)
                scope.import = metadata.GetImportScope(record.Col1);

            if (record.Col2.Length > 0)
            {
                scope.variables = new((int)record.Col2.Length);
                for (uint i = 0; i < record.Col2.Length; i++)
                {
                    VariableDebugInformation variable = ReadLocalVariable(record.Col2.Start + i);
                    if (variable != null)
                        scope.variables.Add(variable);
                }
            }

            if (record.Col3.Length > 0)
            {
                scope.constants = new((int)record.Col3.Length);
                for (uint i = 0; i < record.Col3.Length; i++)
                {
                    ConstantDebugInformation constant = ReadLocalConstant(record.Col3.Start + i);
                    if (constant != null)
                        scope.constants.Add(constant);
                }
            }

            return scope;
        }

        private VariableDebugInformation ReadLocalVariable(uint rid)
        {
            if (!MoveTo(Table.LocalVariable, rid))
                return null;

            VariableAttributes attributes = (VariableAttributes)ReadUInt16();
            ushort index = ReadUInt16();
            string name = ReadString();

            VariableDebugInformation variable = new(index, name) { Attributes = attributes, token = new(TokenType.LocalVariable, rid) };
            variable.custom_infos = GetCustomDebugInformation(variable);
            return variable;
        }

        private ConstantDebugInformation ReadLocalConstant(uint rid)
        {
            if (!MoveTo(Table.LocalConstant, rid))
                return null;

            string name = ReadString();
            SignatureReader signature = ReadSignature(ReadBlobIndex());
            TypeReference type = signature.ReadTypeSignature();

            object value;
            if (type.etype == ElementType.String)
            {
                if (signature.CanReadMore() && signature.buffer[signature.position] != 0xff)
                {
                    byte[] bytes = signature.ReadBytes((int)(signature.sig_length - (signature.position - signature.start)));
                    value = Encoding.Unicode.GetString(bytes, 0, bytes.Length);
                }
                else
                {
                    value = null;
                }
            }
            else if (type.IsTypeOf("System", "Decimal"))
            {
                byte b = signature.ReadByte();
                value = new decimal(signature.ReadInt32(), signature.ReadInt32(), signature.ReadInt32(), (b & 0x80) != 0, (byte)(b & 0x7f));
            }
            else if (type.IsTypeOf("System", "DateTime"))
            {
                value = new DateTime(signature.ReadInt64());
            }
            else if (type.etype == ElementType.Object || type.etype == ElementType.None || type.etype == ElementType.Class || type.etype == ElementType.Array || type.etype == ElementType.GenericInst)
            {
                value = null;
            }
            else
            {
                value = signature.ReadConstantSignature(type.etype);
            }

            ConstantDebugInformation constant = new(name, type, value) { token = new(TokenType.LocalConstant, rid) };
            constant.custom_infos = GetCustomDebugInformation(constant);
            return constant;
        }

        private void InitializeImportScopes()
        {
            if (metadata.ImportScopes != null)
                return;

            int length = MoveTo(Table.ImportScope);

            metadata.ImportScopes = new ImportDebugInformation [length];

            for (int i = 1; i <= length; i++)
            {
                ReadTableIndex(Table.ImportScope);

                ImportDebugInformation import = new();
                import.token = new(TokenType.ImportScope, i);

                SignatureReader signature = ReadSignature(ReadBlobIndex());
                while (signature.CanReadMore())
                    import.Targets.Add(ReadImportTarget(signature));

                metadata.ImportScopes[i - 1] = import;
            }

            MoveTo(Table.ImportScope);

            for (int i = 0; i < length; i++)
            {
                uint parent = ReadTableIndex(Table.ImportScope);

                ReadBlobIndex();

                if (parent != 0)
                    metadata.ImportScopes[i].Parent = metadata.GetImportScope(parent);
            }
        }

        public string ReadUTF8StringBlob(uint signature)
        {
            return ReadStringBlob(signature, Encoding.UTF8);
        }

        private string ReadUnicodeStringBlob(uint signature)
        {
            return ReadStringBlob(signature, Encoding.Unicode);
        }

        private string ReadStringBlob(uint signature, Encoding encoding)
        {
            byte[] blob;
            int index, count;

            GetBlobView(signature, out blob, out index, out count);
            if (count == 0)
                return string.Empty;

            return encoding.GetString(blob, index, count);
        }

        private ImportTarget ReadImportTarget(SignatureReader signature)
        {
            AssemblyNameReference reference = null;
            string @namespace = null;
            string alias = null;
            TypeReference type = null;

            ImportTargetKind kind = (ImportTargetKind)signature.ReadCompressedUInt32();
            switch (kind)
            {
                case ImportTargetKind.ImportNamespace:
                    @namespace = ReadUTF8StringBlob(signature.ReadCompressedUInt32());
                    break;
                case ImportTargetKind.ImportNamespaceInAssembly:
                    reference = metadata.GetAssemblyNameReference(signature.ReadCompressedUInt32());
                    @namespace = ReadUTF8StringBlob(signature.ReadCompressedUInt32());
                    break;
                case ImportTargetKind.ImportType:
                    type = signature.ReadTypeToken();
                    break;
                case ImportTargetKind.ImportXmlNamespaceWithAlias:
                    alias = ReadUTF8StringBlob(signature.ReadCompressedUInt32());
                    @namespace = ReadUTF8StringBlob(signature.ReadCompressedUInt32());
                    break;
                case ImportTargetKind.ImportAlias:
                    alias = ReadUTF8StringBlob(signature.ReadCompressedUInt32());
                    break;
                case ImportTargetKind.DefineAssemblyAlias:
                    alias = ReadUTF8StringBlob(signature.ReadCompressedUInt32());
                    reference = metadata.GetAssemblyNameReference(signature.ReadCompressedUInt32());
                    break;
                case ImportTargetKind.DefineNamespaceAlias:
                    alias = ReadUTF8StringBlob(signature.ReadCompressedUInt32());
                    @namespace = ReadUTF8StringBlob(signature.ReadCompressedUInt32());
                    break;
                case ImportTargetKind.DefineNamespaceInAssemblyAlias:
                    alias = ReadUTF8StringBlob(signature.ReadCompressedUInt32());
                    reference = metadata.GetAssemblyNameReference(signature.ReadCompressedUInt32());
                    @namespace = ReadUTF8StringBlob(signature.ReadCompressedUInt32());
                    break;
                case ImportTargetKind.DefineTypeAlias:
                    alias = ReadUTF8StringBlob(signature.ReadCompressedUInt32());
                    type = signature.ReadTypeToken();
                    break;
            }

            return new(kind)
            {
                alias = alias,
                type = type,
                @namespace = @namespace,
                reference = reference
            };
        }

        private void InitializeStateMachineMethods()
        {
            if (metadata.StateMachineMethods != null)
                return;

            int length = MoveTo(Table.StateMachineMethod);

            metadata.StateMachineMethods = new(length);

            for (int i = 0; i < length; i++)
                metadata.StateMachineMethods.Add(ReadTableIndex(Table.Method), ReadTableIndex(Table.Method));
        }

        public MethodDefinition ReadStateMachineKickoffMethod(MethodDefinition method)
        {
            InitializeStateMachineMethods();

            uint rid;
            if (!metadata.TryGetStateMachineKickOffMethod(method, out rid))
                return null;

            return GetMethodDefinition(rid);
        }

        private void InitializeCustomDebugInformations()
        {
            if (metadata.CustomDebugInformations != null)
                return;

            int length = MoveTo(Table.CustomDebugInformation);

            metadata.CustomDebugInformations = new();

            for (uint i = 1; i <= length; i++)
            {
                MetadataToken token = ReadMetadataToken(CodedIndex.HasCustomDebugInformation);
                Row<Guid, uint, uint> info = new(ReadGuid(), ReadBlobIndex(), i);

                Row<Guid, uint, uint>[] infos;
                metadata.CustomDebugInformations.TryGetValue(token, out infos);
                metadata.CustomDebugInformations[token] = infos.Add(info);
            }
        }

        public Collection<CustomDebugInformation> GetCustomDebugInformation(ICustomDebugInformationProvider provider)
        {
            InitializeCustomDebugInformations();

            Row<Guid, uint, uint>[] rows;
            if (!metadata.CustomDebugInformations.TryGetValue(provider.MetadataToken, out rows))
                return null;

            Collection<CustomDebugInformation> infos = new(rows.Length);

            for (int i = 0; i < rows.Length; i++)
            {
                if (rows[i].Col1 == StateMachineScopeDebugInformation.KindIdentifier)
                {
                    SignatureReader signature = ReadSignature(rows[i].Col2);
                    Collection<StateMachineScope> scopes = new();

                    while (signature.CanReadMore())
                    {
                        int start = signature.ReadInt32();
                        int end = start + signature.ReadInt32();
                        scopes.Add(new(start, end));
                    }

                    StateMachineScopeDebugInformation state_machine = new();
                    state_machine.scopes = scopes;

                    infos.Add(state_machine);
                }
                else if (rows[i].Col1 == AsyncMethodBodyDebugInformation.KindIdentifier)
                {
                    SignatureReader signature = ReadSignature(rows[i].Col2);

                    int catch_offset = signature.ReadInt32() - 1;
                    Collection<InstructionOffset> yields = new();
                    Collection<InstructionOffset> resumes = new();
                    Collection<MethodDefinition> resume_methods = new();

                    while (signature.CanReadMore())
                    {
                        yields.Add(new(signature.ReadInt32()));
                        resumes.Add(new(signature.ReadInt32()));
                        resume_methods.Add(GetMethodDefinition(signature.ReadCompressedUInt32()));
                    }

                    AsyncMethodBodyDebugInformation async_body = new(catch_offset);
                    async_body.yields = yields;
                    async_body.resumes = resumes;
                    async_body.resume_methods = resume_methods;

                    infos.Add(async_body);
                }
                else if (rows[i].Col1 == EmbeddedSourceDebugInformation.KindIdentifier)
                {
                    infos.Add(new EmbeddedSourceDebugInformation(rows[i].Col2, this));
                }
                else if (rows[i].Col1 == SourceLinkDebugInformation.KindIdentifier)
                {
                    infos.Add(new SourceLinkDebugInformation(Encoding.UTF8.GetString(ReadBlob(rows[i].Col2))));
                }
                else
                {
                    infos.Add(new BinaryCustomDebugInformation(rows[i].Col1, ReadBlob(rows[i].Col2)));
                }

                infos[i].token = new(TokenType.CustomDebugInformation, rows[i].Col3);
            }

            return infos;
        }

        public byte[] ReadRawEmbeddedSourceDebugInformation(uint index)
        {
            SignatureReader signature = ReadSignature(index);
            return signature.ReadBytes((int)signature.sig_length);
        }

        public Row<byte[], bool> ReadEmbeddedSourceDebugInformation(uint index)
        {
            SignatureReader signature = ReadSignature(index);
            int format = signature.ReadInt32();
            uint length = signature.sig_length - 4;

            if (format == 0)
            {
                return new(signature.ReadBytes((int)length), false);
            }
            else if (format > 0)
            {
                MemoryStream compressed_stream = new(signature.ReadBytes((int)length));
                byte[] decompressed_document = new byte [format]; // if positive, format is the decompressed length of the document
                MemoryStream decompressed_stream = new(decompressed_document);

                using (DeflateStream deflate_stream = new(compressed_stream, CompressionMode.Decompress, leaveOpen: true))
                {
                    deflate_stream.CopyTo(decompressed_stream);
                }

                return new(decompressed_document, true);
            }
            else
            {
                throw new NotSupportedException();
            }
        }
    }

    internal sealed class SignatureReader : ByteBuffer
    {
        private readonly MetadataReader reader;
        internal readonly uint start, sig_length;
        private TypeSystem TypeSystem
        {
            get { return reader.module.TypeSystem; }
        }

        public SignatureReader(uint blob, MetadataReader reader) : base(reader.image.BlobHeap.data)
        {
            this.reader = reader;
            position = (int)blob;
            sig_length = ReadCompressedUInt32();
            start = (uint)position;
        }

        private MetadataToken ReadTypeTokenSignature()
        {
            return CodedIndex.TypeDefOrRef.GetMetadataToken(ReadCompressedUInt32());
        }

        private GenericParameter GetGenericParameter(GenericParameterType type, uint var)
        {
            IGenericContext context = reader.context;
            int index = (int)var;

            if (context == null)
                return GetUnboundGenericParameter(type, index);

            IGenericParameterProvider provider;

            switch (type)
            {
                case GenericParameterType.Type:
                    provider = context.Type;
                    break;
                case GenericParameterType.Method:
                    provider = context.Method;
                    break;
                default:
                    throw new NotSupportedException();
            }

            if (!context.IsDefinition)
                CheckGenericContext(provider, index);

            if (index >= provider.GenericParameters.Count)
                return GetUnboundGenericParameter(type, index);

            return provider.GenericParameters[index];
        }

        private GenericParameter GetUnboundGenericParameter(GenericParameterType type, int index)
        {
            return new(index, type, reader.module);
        }

        private static void CheckGenericContext(IGenericParameterProvider owner, int index)
        {
            Collection<GenericParameter> owner_parameters = owner.GenericParameters;

            for (int i = owner_parameters.Count; i <= index; i++)
                owner_parameters.Add(new(owner));
        }

        public void ReadGenericInstanceSignature(IGenericParameterProvider provider, IGenericInstance instance, uint arity)
        {
            if (!provider.IsDefinition)
                CheckGenericContext(provider, (int)arity - 1);

            Collection<TypeReference> instance_arguments = instance.GenericArguments;

            for (int i = 0; i < arity; i++)
                instance_arguments.Add(ReadTypeSignature());
        }

        private ArrayType ReadArrayTypeSignature()
        {
            ArrayType array = new(ReadTypeSignature());

            uint rank = ReadCompressedUInt32();

            uint[] sizes = new uint [ReadCompressedUInt32()];
            for (int i = 0; i < sizes.Length; i++)
                sizes[i] = ReadCompressedUInt32();

            int[] low_bounds = new int [ReadCompressedUInt32()];
            for (int i = 0; i < low_bounds.Length; i++)
                low_bounds[i] = ReadCompressedInt32();

            array.Dimensions.Clear();

            for (int i = 0; i < rank; i++)
            {
                int? lower = null, upper = null;

                if (i < low_bounds.Length)
                    lower = low_bounds[i];

                if (i < sizes.Length)
                    upper = lower + (int)sizes[i] - 1;

                array.Dimensions.Add(new(lower, upper));
            }

            return array;
        }

        private TypeReference GetTypeDefOrRef(MetadataToken token)
        {
            return reader.GetTypeDefOrRef(token);
        }

        public TypeReference ReadTypeSignature()
        {
            return ReadTypeSignature((ElementType)ReadByte());
        }

        public TypeReference ReadTypeToken()
        {
            return GetTypeDefOrRef(ReadTypeTokenSignature());
        }

        private TypeReference ReadTypeSignature(ElementType etype)
        {
            switch (etype)
            {
                case ElementType.ValueType:
                {
                    TypeReference value_type = GetTypeDefOrRef(ReadTypeTokenSignature());
                    value_type.KnownValueType();
                    return value_type;
                }
                case ElementType.Class:
                    return GetTypeDefOrRef(ReadTypeTokenSignature());
                case ElementType.Ptr:
                    return new PointerType(ReadTypeSignature());
                case ElementType.FnPtr:
                {
                    FunctionPointerType fptr = new();
                    ReadMethodSignature(fptr);
                    return fptr;
                }
                case ElementType.ByRef:
                    return new ByReferenceType(ReadTypeSignature());
                case ElementType.Pinned:
                    return new PinnedType(ReadTypeSignature());
                case ElementType.SzArray:
                    return new ArrayType(ReadTypeSignature());
                case ElementType.Array:
                    return ReadArrayTypeSignature();
                case ElementType.CModOpt:
                    return new OptionalModifierType(GetTypeDefOrRef(ReadTypeTokenSignature()), ReadTypeSignature());
                case ElementType.CModReqD:
                    return new RequiredModifierType(GetTypeDefOrRef(ReadTypeTokenSignature()), ReadTypeSignature());
                case ElementType.Sentinel:
                    return new SentinelType(ReadTypeSignature());
                case ElementType.Var:
                    return GetGenericParameter(GenericParameterType.Type, ReadCompressedUInt32());
                case ElementType.MVar:
                    return GetGenericParameter(GenericParameterType.Method, ReadCompressedUInt32());
                case ElementType.GenericInst:
                {
                    bool is_value_type = ReadByte() == (byte)ElementType.ValueType;
                    TypeReference element_type = GetTypeDefOrRef(ReadTypeTokenSignature());

                    uint arity = ReadCompressedUInt32();
                    GenericInstanceType generic_instance = new(element_type, (int)arity);

                    ReadGenericInstanceSignature(element_type, generic_instance, arity);

                    if (is_value_type)
                    {
                        generic_instance.KnownValueType();
                        element_type.GetElementType().KnownValueType();
                    }

                    return generic_instance;
                }
                case ElementType.Object:
                    return TypeSystem.Object;
                case ElementType.Void:
                    return TypeSystem.Void;
                case ElementType.TypedByRef:
                    return TypeSystem.TypedReference;
                case ElementType.I:
                    return TypeSystem.IntPtr;
                case ElementType.U:
                    return TypeSystem.UIntPtr;
                default:
                    return GetPrimitiveType(etype);
            }
        }

        public void ReadMethodSignature(IMethodSignature method)
        {
            byte calling_convention = ReadByte();

            const byte has_this = 0x20;
            const byte explicit_this = 0x40;

            if ((calling_convention & has_this) != 0)
            {
                method.HasThis = true;
                calling_convention = (byte)(calling_convention & ~has_this);
            }

            if ((calling_convention & explicit_this) != 0)
            {
                method.ExplicitThis = true;
                calling_convention = (byte)(calling_convention & ~explicit_this);
            }

            method.CallingConvention = (MethodCallingConvention)calling_convention;

            MethodReference generic_context = method as MethodReference;
            if (generic_context != null && !generic_context.DeclaringType.IsArray)
                reader.context = generic_context;

            if ((calling_convention & 0x10) != 0)
            {
                uint arity = ReadCompressedUInt32();

                if (generic_context != null && !generic_context.IsDefinition)
                    CheckGenericContext(generic_context, (int)arity - 1);
            }

            uint param_count = ReadCompressedUInt32();

            method.MethodReturnType.ReturnType = ReadTypeSignature();

            if (param_count == 0)
                return;

            Collection<ParameterDefinition> parameters;

            MethodReference method_ref = method as MethodReference;
            if (method_ref != null)
                parameters = method_ref.parameters = new(method, (int)param_count);
            else
                parameters = method.Parameters;

            for (int i = 0; i < param_count; i++)
                parameters.Add(new(ReadTypeSignature()));
        }

        public object ReadConstantSignature(ElementType type)
        {
            return ReadPrimitiveValue(type);
        }

        public void ReadCustomAttributeConstructorArguments(CustomAttribute attribute, Collection<ParameterDefinition> parameters)
        {
            int count = parameters.Count;
            if (count == 0)
                return;

            attribute.arguments = new(count);

            for (int i = 0; i < count; i++)
                attribute.arguments.Add(ReadCustomAttributeFixedArgument(parameters[i].ParameterType));
        }

        private CustomAttributeArgument ReadCustomAttributeFixedArgument(TypeReference type)
        {
            if (type.IsArray)
                return ReadCustomAttributeFixedArrayArgument((ArrayType)type);

            return ReadCustomAttributeElement(type);
        }

        public void ReadCustomAttributeNamedArguments(ushort count, ref Collection<CustomAttributeNamedArgument> fields, ref Collection<CustomAttributeNamedArgument> properties)
        {
            for (int i = 0; i < count; i++)
            {
                if (!CanReadMore())
                    return;
                ReadCustomAttributeNamedArgument(ref fields, ref properties);
            }
        }

        private void ReadCustomAttributeNamedArgument(ref Collection<CustomAttributeNamedArgument> fields, ref Collection<CustomAttributeNamedArgument> properties)
        {
            byte kind = ReadByte();
            TypeReference type = ReadCustomAttributeFieldOrPropType();
            string name = ReadUTF8String();

            Collection<CustomAttributeNamedArgument> container;
            switch (kind)
            {
                case 0x53:
                    container = GetCustomAttributeNamedArgumentCollection(ref fields);
                    break;
                case 0x54:
                    container = GetCustomAttributeNamedArgumentCollection(ref properties);
                    break;
                default:
                    throw new NotSupportedException();
            }

            container.Add(new(name, ReadCustomAttributeFixedArgument(type)));
        }

        private static Collection<CustomAttributeNamedArgument> GetCustomAttributeNamedArgumentCollection(ref Collection<CustomAttributeNamedArgument> collection)
        {
            if (collection != null)
                return collection;

            return collection = new();
        }

        private CustomAttributeArgument ReadCustomAttributeFixedArrayArgument(ArrayType type)
        {
            uint length = ReadUInt32();

            if (length == 0xffffffff)
                return new(type, null);

            if (length == 0)
                return new(type, Empty<CustomAttributeArgument>.Array);

            CustomAttributeArgument[] arguments = new CustomAttributeArgument [length];
            TypeReference element_type = type.ElementType;

            for (int i = 0; i < length; i++)
                arguments[i] = ReadCustomAttributeElement(element_type);

            return new(type, arguments);
        }

        private CustomAttributeArgument ReadCustomAttributeElement(TypeReference type)
        {
            if (type.IsArray)
                return ReadCustomAttributeFixedArrayArgument((ArrayType)type);

            return new(type, type.etype == ElementType.Object ? ReadCustomAttributeElement(ReadCustomAttributeFieldOrPropType()) : ReadCustomAttributeElementValue(type));
        }

        private object ReadCustomAttributeElementValue(TypeReference type)
        {
            ElementType etype = type.etype;

            switch (etype)
            {
                case ElementType.String:
                    return ReadUTF8String();
                case ElementType.None:
                    if (type.IsTypeOf("System", "Type"))
                        return ReadTypeReference();

                    return ReadCustomAttributeEnum(type);
                default:
                    return ReadPrimitiveValue(etype);
            }
        }

        private object ReadPrimitiveValue(ElementType type)
        {
            switch (type)
            {
                case ElementType.Boolean:
                    return ReadByte() == 1;
                case ElementType.I1:
                    return (sbyte)ReadByte();
                case ElementType.U1:
                    return ReadByte();
                case ElementType.Char:
                    return (char)ReadUInt16();
                case ElementType.I2:
                    return ReadInt16();
                case ElementType.U2:
                    return ReadUInt16();
                case ElementType.I4:
                    return ReadInt32();
                case ElementType.U4:
                    return ReadUInt32();
                case ElementType.I8:
                    return ReadInt64();
                case ElementType.U8:
                    return ReadUInt64();
                case ElementType.R4:
                    return ReadSingle();
                case ElementType.R8:
                    return ReadDouble();
                default:
                    throw new NotImplementedException(type.ToString());
            }
        }

        private TypeReference GetPrimitiveType(ElementType etype)
        {
            switch (etype)
            {
                case ElementType.Boolean:
                    return TypeSystem.Boolean;
                case ElementType.Char:
                    return TypeSystem.Char;
                case ElementType.I1:
                    return TypeSystem.SByte;
                case ElementType.U1:
                    return TypeSystem.Byte;
                case ElementType.I2:
                    return TypeSystem.Int16;
                case ElementType.U2:
                    return TypeSystem.UInt16;
                case ElementType.I4:
                    return TypeSystem.Int32;
                case ElementType.U4:
                    return TypeSystem.UInt32;
                case ElementType.I8:
                    return TypeSystem.Int64;
                case ElementType.U8:
                    return TypeSystem.UInt64;
                case ElementType.R4:
                    return TypeSystem.Single;
                case ElementType.R8:
                    return TypeSystem.Double;
                case ElementType.String:
                    return TypeSystem.String;
                default:
                    throw new NotImplementedException(etype.ToString());
            }
        }

        private TypeReference ReadCustomAttributeFieldOrPropType()
        {
            ElementType etype = (ElementType)ReadByte();

            switch (etype)
            {
                case ElementType.Boxed:
                    return TypeSystem.Object;
                case ElementType.SzArray:
                    return new ArrayType(ReadCustomAttributeFieldOrPropType());
                case ElementType.Enum:
                    return ReadTypeReference();
                case ElementType.Type:
                    return TypeSystem.LookupType("System", "Type");
                default:
                    return GetPrimitiveType(etype);
            }
        }

        public TypeReference ReadTypeReference()
        {
            return TypeParser.ParseType(reader.module, ReadUTF8String());
        }

        private object ReadCustomAttributeEnum(TypeReference enum_type)
        {
            TypeDefinition type = enum_type.CheckedResolve();
            if (!type.IsEnum)
                throw new ArgumentException();

            return ReadCustomAttributeElementValue(type.GetEnumUnderlyingType());
        }

        public SecurityAttribute ReadSecurityAttribute()
        {
            SecurityAttribute attribute = new(ReadTypeReference());

            ReadCompressedUInt32();

            ReadCustomAttributeNamedArguments((ushort)ReadCompressedUInt32(), ref attribute.fields, ref attribute.properties);

            return attribute;
        }

        public MarshalInfo ReadMarshalInfo()
        {
            NativeType native = ReadNativeType();
            switch (native)
            {
                case NativeType.Array:
                {
                    ArrayMarshalInfo array = new();
                    if (CanReadMore())
                        array.element_type = ReadNativeType();
                    if (CanReadMore())
                        array.size_parameter_index = (int)ReadCompressedUInt32();
                    if (CanReadMore())
                        array.size = (int)ReadCompressedUInt32();
                    if (CanReadMore())
                        array.size_parameter_multiplier = (int)ReadCompressedUInt32();
                    return array;
                }
                case NativeType.SafeArray:
                {
                    SafeArrayMarshalInfo array = new();
                    if (CanReadMore())
                        array.element_type = ReadVariantType();
                    return array;
                }
                case NativeType.FixedArray:
                {
                    FixedArrayMarshalInfo array = new();
                    if (CanReadMore())
                        array.size = (int)ReadCompressedUInt32();
                    if (CanReadMore())
                        array.element_type = ReadNativeType();
                    return array;
                }
                case NativeType.FixedSysString:
                {
                    FixedSysStringMarshalInfo sys_string = new();
                    if (CanReadMore())
                        sys_string.size = (int)ReadCompressedUInt32();
                    return sys_string;
                }
                case NativeType.CustomMarshaler:
                {
                    CustomMarshalInfo marshaler = new();
                    string guid_value = ReadUTF8String();
                    marshaler.guid = !string.IsNullOrEmpty(guid_value) ? new(guid_value) : Guid.Empty;
                    marshaler.unmanaged_type = ReadUTF8String();
                    marshaler.managed_type = ReadTypeReference();
                    marshaler.cookie = ReadUTF8String();
                    return marshaler;
                }
                default:
                    return new(native);
            }
        }

        private NativeType ReadNativeType()
        {
            return (NativeType)ReadByte();
        }

        private VariantType ReadVariantType()
        {
            return (VariantType)ReadByte();
        }

        private string ReadUTF8String()
        {
            if (buffer[position] == 0xff)
            {
                position++;
                return null;
            }

            int length = (int)ReadCompressedUInt32();
            if (length == 0)
                return string.Empty;

            if (position + length > buffer.Length)
                return string.Empty;

            string @string = Encoding.UTF8.GetString(buffer, position, length);

            position += length;
            return @string;
        }

        public string ReadDocumentName()
        {
            char separator = (char)buffer[position];
            position++;

            StringBuilder builder = new();
            for (int i = 0; CanReadMore(); i++)
            {
                if (i > 0 && separator != 0)
                    builder.Append(separator);

                uint part = ReadCompressedUInt32();
                if (part != 0)
                    builder.Append(reader.ReadUTF8StringBlob(part));
            }

            return builder.ToString();
        }

        public Collection<SequencePoint> ReadSequencePoints(Document document)
        {
            ReadCompressedUInt32(); // local_sig_token

            if (document == null)
                document = reader.GetDocument(ReadCompressedUInt32());

            int offset = 0;
            int start_line = 0;
            int start_column = 0;
            bool first_non_hidden = true;

            // there's about 5 compressed int32's per sequenec points.  we don't know exactly how many
            // but let's take a conservative guess so we dont end up reallocating the sequence_points collection
            // as it grows.
            long bytes_remaining_for_sequencepoints = sig_length - (position - start);
            int estimated_sequencepoint_amount = (int)bytes_remaining_for_sequencepoints / 5;
            Collection<SequencePoint> sequence_points = new(estimated_sequencepoint_amount);

            for (int i = 0; CanReadMore(); i++)
            {
                int delta_il = (int)ReadCompressedUInt32();
                if (i > 0 && delta_il == 0)
                {
                    document = reader.GetDocument(ReadCompressedUInt32());
                    continue;
                }

                offset += delta_il;

                int delta_lines = (int)ReadCompressedUInt32();
                int delta_columns = delta_lines == 0 ? (int)ReadCompressedUInt32() : ReadCompressedInt32();

                if (delta_lines == 0 && delta_columns == 0)
                {
                    sequence_points.Add(new(offset, document)
                    {
                        StartLine = 0xfeefee,
                        EndLine = 0xfeefee,
                        StartColumn = 0,
                        EndColumn = 0
                    });
                    continue;
                }

                if (first_non_hidden)
                {
                    start_line = (int)ReadCompressedUInt32();
                    start_column = (int)ReadCompressedUInt32();
                }
                else
                {
                    start_line += ReadCompressedInt32();
                    start_column += ReadCompressedInt32();
                }

                sequence_points.Add(new(offset, document)
                {
                    StartLine = start_line,
                    StartColumn = start_column,
                    EndLine = start_line + delta_lines,
                    EndColumn = start_column + delta_columns
                });
                first_non_hidden = false;
            }

            return sequence_points;
        }

        public bool CanReadMore()
        {
            return position - start < sig_length;
        }
    }
}