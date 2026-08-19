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
using BlobIndex = System.UInt32;
using CodedRID = System.UInt32;
using GuidIndex = System.UInt32;
using RID = System.UInt32;
using RVA = System.UInt32;
using StringIndex = System.UInt32;

namespace MonoFN.Cecil
{
    using AssemblyRefRow = Row<ushort, ushort, ushort, ushort, AssemblyAttributes, uint, uint, uint, uint>;
    using AssemblyRow = Row<AssemblyHashAlgorithm, ushort, ushort, ushort, ushort, AssemblyAttributes, uint, uint, uint>;
    using ClassLayoutRow = Row<ushort, uint, RID>;
    using ConstantRow = Row<ElementType, CodedRID, BlobIndex>;
    using CustomAttributeRow = Row<CodedRID, CodedRID, BlobIndex>;
    using CustomDebugInformationRow = Row<CodedRID, GuidIndex, BlobIndex>;
    using DeclSecurityRow = Row<SecurityAction, CodedRID, BlobIndex>;
    using DocumentRow = Row<BlobIndex, GuidIndex, BlobIndex, GuidIndex>;
    using EventMapRow = Row<RID, RID>;
    using EventRow = Row<EventAttributes, StringIndex, CodedRID>;
    using ExportedTypeRow = Row<TypeAttributes, uint, StringIndex, StringIndex, CodedRID>;
    using FieldLayoutRow = Row<uint, RID>;
    using FieldMarshalRow = Row<CodedRID, BlobIndex>;
    using FieldRow = Row<FieldAttributes, StringIndex, BlobIndex>;
    using FieldRVARow = Row<RVA, RID>;
    using FileRow = Row<FileAttributes, StringIndex, BlobIndex>;
    using GenericParamConstraintRow = Row<RID, CodedRID>;
    using GenericParamRow = Row<ushort, GenericParameterAttributes, CodedRID, StringIndex>;
    using ImplMapRow = Row<PInvokeAttributes, CodedRID, StringIndex, RID>;
    using ImportScopeRow = Row<RID, BlobIndex>;
    using InterfaceImplRow = Row<uint, CodedRID>;
    using LocalConstantRow = Row<StringIndex, BlobIndex>;
    using LocalScopeRow = Row<RID, RID, RID, RID, uint, uint>;
    using LocalVariableRow = Row<VariableAttributes, ushort, StringIndex>;
    using ManifestResourceRow = Row<uint, ManifestResourceAttributes, StringIndex, CodedRID>;
    using MemberRefRow = Row<CodedRID, StringIndex, BlobIndex>;
    using MethodDebugInformationRow = Row<RID, BlobIndex>;
    using MethodImplRow = Row<RID, CodedRID, CodedRID>;
    using MethodRow = Row<RVA, MethodImplAttributes, MethodAttributes, StringIndex, BlobIndex, RID>;
    using MethodSemanticsRow = Row<MethodSemanticsAttributes, RID, CodedRID>;
    using MethodSpecRow = Row<CodedRID, BlobIndex>;
    using ModuleRow = Row<StringIndex, GuidIndex>;
    using NestedClassRow = Row<RID, RID>;
    using ParamRow = Row<ParameterAttributes, ushort, StringIndex>;
    using PropertyMapRow = Row<RID, RID>;
    using PropertyRow = Row<PropertyAttributes, StringIndex, BlobIndex>;
    using StateMachineMethodRow = Row<RID, RID>;
    using TypeDefRow = Row<TypeAttributes, StringIndex, StringIndex, CodedRID, RID, RID>;
    using TypeRefRow = Row<CodedRID, StringIndex, StringIndex>;

    internal static class ModuleWriter
    {
        public static void WriteModule(ModuleDefinition module, Disposable<Stream> stream, WriterParameters parameters)
        {
            using (stream)
            {
                Write(module, stream, parameters);
            }
        }

        private static void Write(ModuleDefinition module, Disposable<Stream> stream, WriterParameters parameters)
        {
            if ((module.Attributes & ModuleAttributes.ILOnly) == 0)
                throw new NotSupportedException("Writing mixed-mode assemblies is not supported");

            if (module.HasImage && module.ReadingMode == ReadingMode.Deferred)
            {
                ImmediateModuleReader immediate_reader = new(module.Image);
                immediate_reader.ReadModule(module, resolve_attributes: false);
                immediate_reader.ReadSymbols(module);
            }

            module.MetadataSystem.Clear();

            if (module.symbol_reader != null)
                module.symbol_reader.Dispose();

            AssemblyNameDefinition name = module.assembly != null && module.kind != ModuleKind.NetModule ? module.assembly.Name : null;
            string fq_name = stream.value.GetFileName();
            uint timestamp = parameters.Timestamp ?? module.timestamp;
            ISymbolWriterProvider symbol_writer_provider = parameters.SymbolWriterProvider;

            if (symbol_writer_provider == null && parameters.WriteSymbols)
                symbol_writer_provider = new DefaultSymbolWriterProvider();

            if (parameters.HasStrongNameKey && name != null)
            {
                name.PublicKey = CryptoService.GetPublicKey(parameters);
                module.Attributes |= ModuleAttributes.StrongNameSigned;
            }

            if (parameters.DeterministicMvid)
                module.Mvid = Guid.Empty;

            MetadataBuilder metadata = new(module, fq_name, timestamp, symbol_writer_provider);
            try
            {
                module.metadata_builder = metadata;

                using (ISymbolWriter symbol_writer = GetSymbolWriter(module, fq_name, symbol_writer_provider, parameters))
                {
                    metadata.SetSymbolWriter(symbol_writer);
                    BuildMetadata(module, metadata);

                    if (parameters.DeterministicMvid)
                        metadata.ComputeDeterministicMvid();

                    ImageWriter writer = ImageWriter.CreateWriter(module, metadata, stream);
                    stream.value.SetLength(0);
                    writer.WriteImage();

                    if (parameters.HasStrongNameKey)
                        CryptoService.StrongName(stream.value, writer, parameters);
                }
            }
            finally
            {
                module.metadata_builder = null;
            }
        }

        private static void BuildMetadata(ModuleDefinition module, MetadataBuilder metadata)
        {
            if (!module.HasImage)
            {
                metadata.BuildMetadata();
                return;
            }

            module.Read(metadata, (builder, _) =>
            {
                builder.BuildMetadata();
                return builder;
            });
        }

        private static ISymbolWriter GetSymbolWriter(ModuleDefinition module, string fq_name, ISymbolWriterProvider symbol_writer_provider, WriterParameters parameters)
        {
            if (symbol_writer_provider == null)
                return null;

            if (parameters.SymbolStream != null)
                return symbol_writer_provider.GetSymbolWriter(module, parameters.SymbolStream);

            return symbol_writer_provider.GetSymbolWriter(module, fq_name);
        }
    }

    internal abstract class MetadataTable
    {
        public abstract int Length { get; }
        public bool IsLarge
        {
            get { return Length > ushort.MaxValue; }
        }
        public abstract void Write(TableHeapBuffer buffer);
        public abstract void Sort();
    }

    internal abstract class OneRowTable<TRow> : MetadataTable where TRow : struct
    {
        internal TRow row;
        public sealed override int Length
        {
            get { return 1; }
        }
        public sealed override void Sort() { }
    }

    internal abstract class MetadataTable<TRow> : MetadataTable where TRow : struct
    {
        internal TRow[] rows = new TRow [2];
        internal int length;
        public sealed override int Length
        {
            get { return length; }
        }

        public int AddRow(TRow row)
        {
            if (rows.Length == length)
                Grow();

            rows[length++] = row;
            return length;
        }

        private void Grow()
        {
            TRow[] rows = new TRow [this.rows.Length * 2];
            Array.Copy(this.rows, rows, this.rows.Length);
            this.rows = rows;
        }

        public override void Sort() { }
    }

    internal abstract class SortedTable<TRow> : MetadataTable<TRow>, IComparer<TRow> where TRow : struct
    {
        public sealed override void Sort()
        {
            MergeSort<TRow>.Sort(rows, 0, length, this);
        }

        protected static int Compare(uint x, uint y)
        {
            return x == y ? 0 : x > y ? 1 : -1;
        }

        public abstract int Compare(TRow x, TRow y);
    }

    internal sealed class ModuleTable : OneRowTable<ModuleRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            buffer.WriteUInt16(0); // Generation
            buffer.WriteString(row.Col1); // Name
            buffer.WriteGuid(row.Col2); // Mvid
            buffer.WriteUInt16(0); // EncId
            buffer.WriteUInt16(0); // EncBaseId
        }
    }

    internal sealed class TypeRefTable : MetadataTable<TypeRefRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteCodedRID(rows[i].Col1, CodedIndex.ResolutionScope); // Scope
                buffer.WriteString(rows[i].Col2); // Name
                buffer.WriteString(rows[i].Col3); // Namespace
            }
        }
    }

    internal sealed class TypeDefTable : MetadataTable<TypeDefRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteUInt32((uint)rows[i].Col1); // Attributes
                buffer.WriteString(rows[i].Col2); // Name
                buffer.WriteString(rows[i].Col3); // Namespace
                buffer.WriteCodedRID(rows[i].Col4, CodedIndex.TypeDefOrRef); // Extends
                buffer.WriteRID(rows[i].Col5, Table.Field); // FieldList
                buffer.WriteRID(rows[i].Col6, Table.Method); // MethodList
            }
        }
    }

    internal sealed class FieldTable : MetadataTable<FieldRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteUInt16((ushort)rows[i].Col1); // Attributes
                buffer.WriteString(rows[i].Col2); // Name
                buffer.WriteBlob(rows[i].Col3); // Signature
            }
        }
    }

    internal sealed class MethodTable : MetadataTable<MethodRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteUInt32(rows[i].Col1); // RVA
                buffer.WriteUInt16((ushort)rows[i].Col2); // ImplFlags
                buffer.WriteUInt16((ushort)rows[i].Col3); // Flags
                buffer.WriteString(rows[i].Col4); // Name
                buffer.WriteBlob(rows[i].Col5); // Signature
                buffer.WriteRID(rows[i].Col6, Table.Param); // ParamList
            }
        }
    }

    internal sealed class ParamTable : MetadataTable<ParamRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteUInt16((ushort)rows[i].Col1); // Attributes
                buffer.WriteUInt16(rows[i].Col2); // Sequence
                buffer.WriteString(rows[i].Col3); // Name
            }
        }
    }

    internal sealed class InterfaceImplTable : MetadataTable<InterfaceImplRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteRID(rows[i].Col1, Table.TypeDef); // Class
                buffer.WriteCodedRID(rows[i].Col2, CodedIndex.TypeDefOrRef); // Interface
            }
        }

        /*public override int Compare (InterfaceImplRow x, InterfaceImplRow y)
        {
            return (int) (x.Col1 == y.Col1 ? y.Col2 - x.Col2 : x.Col1 - y.Col1);
        }*/
    }

    internal sealed class MemberRefTable : MetadataTable<MemberRefRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteCodedRID(rows[i].Col1, CodedIndex.MemberRefParent);
                buffer.WriteString(rows[i].Col2);
                buffer.WriteBlob(rows[i].Col3);
            }
        }
    }

    internal sealed class ConstantTable : SortedTable<ConstantRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteUInt16((ushort)rows[i].Col1);
                buffer.WriteCodedRID(rows[i].Col2, CodedIndex.HasConstant);
                buffer.WriteBlob(rows[i].Col3);
            }
        }

        public override int Compare(ConstantRow x, ConstantRow y)
        {
            return Compare(x.Col2, y.Col2);
        }
    }

    internal sealed class CustomAttributeTable : SortedTable<CustomAttributeRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteCodedRID(rows[i].Col1, CodedIndex.HasCustomAttribute); // Parent
                buffer.WriteCodedRID(rows[i].Col2, CodedIndex.CustomAttributeType); // Type
                buffer.WriteBlob(rows[i].Col3);
            }
        }

        public override int Compare(CustomAttributeRow x, CustomAttributeRow y)
        {
            return Compare(x.Col1, y.Col1);
        }
    }

    internal sealed class FieldMarshalTable : SortedTable<FieldMarshalRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteCodedRID(rows[i].Col1, CodedIndex.HasFieldMarshal);
                buffer.WriteBlob(rows[i].Col2);
            }
        }

        public override int Compare(FieldMarshalRow x, FieldMarshalRow y)
        {
            return Compare(x.Col1, y.Col1);
        }
    }

    internal sealed class DeclSecurityTable : SortedTable<DeclSecurityRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteUInt16((ushort)rows[i].Col1);
                buffer.WriteCodedRID(rows[i].Col2, CodedIndex.HasDeclSecurity);
                buffer.WriteBlob(rows[i].Col3);
            }
        }

        public override int Compare(DeclSecurityRow x, DeclSecurityRow y)
        {
            return Compare(x.Col2, y.Col2);
        }
    }

    internal sealed class ClassLayoutTable : SortedTable<ClassLayoutRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteUInt16(rows[i].Col1); // PackingSize
                buffer.WriteUInt32(rows[i].Col2); // ClassSize
                buffer.WriteRID(rows[i].Col3, Table.TypeDef); // Parent
            }
        }

        public override int Compare(ClassLayoutRow x, ClassLayoutRow y)
        {
            return Compare(x.Col3, y.Col3);
        }
    }

    internal sealed class FieldLayoutTable : SortedTable<FieldLayoutRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteUInt32(rows[i].Col1); // Offset
                buffer.WriteRID(rows[i].Col2, Table.Field); // Parent
            }
        }

        public override int Compare(FieldLayoutRow x, FieldLayoutRow y)
        {
            return Compare(x.Col2, y.Col2);
        }
    }

    internal sealed class StandAloneSigTable : MetadataTable<uint>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
                buffer.WriteBlob(rows[i]);
        }
    }

    internal sealed class EventMapTable : MetadataTable<EventMapRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteRID(rows[i].Col1, Table.TypeDef); // Parent
                buffer.WriteRID(rows[i].Col2, Table.Event); // EventList
            }
        }
    }

    internal sealed class EventTable : MetadataTable<EventRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteUInt16((ushort)rows[i].Col1); // Flags
                buffer.WriteString(rows[i].Col2); // Name
                buffer.WriteCodedRID(rows[i].Col3, CodedIndex.TypeDefOrRef); // EventType
            }
        }
    }

    internal sealed class PropertyMapTable : MetadataTable<PropertyMapRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteRID(rows[i].Col1, Table.TypeDef); // Parent
                buffer.WriteRID(rows[i].Col2, Table.Property); // PropertyList
            }
        }
    }

    internal sealed class PropertyTable : MetadataTable<PropertyRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteUInt16((ushort)rows[i].Col1); // Flags
                buffer.WriteString(rows[i].Col2); // Name
                buffer.WriteBlob(rows[i].Col3); // Type
            }
        }
    }

    internal sealed class MethodSemanticsTable : SortedTable<MethodSemanticsRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteUInt16((ushort)rows[i].Col1); // Flags
                buffer.WriteRID(rows[i].Col2, Table.Method); // Method
                buffer.WriteCodedRID(rows[i].Col3, CodedIndex.HasSemantics); // Association
            }
        }

        public override int Compare(MethodSemanticsRow x, MethodSemanticsRow y)
        {
            return Compare(x.Col3, y.Col3);
        }
    }

    internal sealed class MethodImplTable : MetadataTable<MethodImplRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteRID(rows[i].Col1, Table.TypeDef); // Class
                buffer.WriteCodedRID(rows[i].Col2, CodedIndex.MethodDefOrRef); // MethodBody
                buffer.WriteCodedRID(rows[i].Col3, CodedIndex.MethodDefOrRef); // MethodDeclaration
            }
        }
    }

    internal sealed class ModuleRefTable : MetadataTable<uint>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
                buffer.WriteString(rows[i]); // Name
        }
    }

    internal sealed class TypeSpecTable : MetadataTable<uint>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
                buffer.WriteBlob(rows[i]); // Signature
        }
    }

    internal sealed class ImplMapTable : SortedTable<ImplMapRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteUInt16((ushort)rows[i].Col1); // Flags
                buffer.WriteCodedRID(rows[i].Col2, CodedIndex.MemberForwarded); // MemberForwarded
                buffer.WriteString(rows[i].Col3); // ImportName
                buffer.WriteRID(rows[i].Col4, Table.ModuleRef); // ImportScope
            }
        }

        public override int Compare(ImplMapRow x, ImplMapRow y)
        {
            return Compare(x.Col2, y.Col2);
        }
    }

    internal sealed class FieldRVATable : SortedTable<FieldRVARow>
    {
        internal int position;

        public override void Write(TableHeapBuffer buffer)
        {
            position = buffer.position;
            for (int i = 0; i < length; i++)
            {
                buffer.WriteUInt32(rows[i].Col1); // RVA
                buffer.WriteRID(rows[i].Col2, Table.Field); // Field
            }
        }

        public override int Compare(FieldRVARow x, FieldRVARow y)
        {
            return Compare(x.Col2, y.Col2);
        }
    }

    internal sealed class AssemblyTable : OneRowTable<AssemblyRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            buffer.WriteUInt32((uint)row.Col1); // AssemblyHashAlgorithm
            buffer.WriteUInt16(row.Col2); // MajorVersion
            buffer.WriteUInt16(row.Col3); // MinorVersion
            buffer.WriteUInt16(row.Col4); // Build
            buffer.WriteUInt16(row.Col5); // Revision
            buffer.WriteUInt32((uint)row.Col6); // Flags
            buffer.WriteBlob(row.Col7); // PublicKey
            buffer.WriteString(row.Col8); // Name
            buffer.WriteString(row.Col9); // Culture
        }
    }

    internal sealed class AssemblyRefTable : MetadataTable<AssemblyRefRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteUInt16(rows[i].Col1); // MajorVersion
                buffer.WriteUInt16(rows[i].Col2); // MinorVersion
                buffer.WriteUInt16(rows[i].Col3); // Build
                buffer.WriteUInt16(rows[i].Col4); // Revision
                buffer.WriteUInt32((uint)rows[i].Col5); // Flags
                buffer.WriteBlob(rows[i].Col6); // PublicKeyOrToken
                buffer.WriteString(rows[i].Col7); // Name
                buffer.WriteString(rows[i].Col8); // Culture
                buffer.WriteBlob(rows[i].Col9); // Hash
            }
        }
    }

    internal sealed class FileTable : MetadataTable<FileRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteUInt32((uint)rows[i].Col1);
                buffer.WriteString(rows[i].Col2);
                buffer.WriteBlob(rows[i].Col3);
            }
        }
    }

    internal sealed class ExportedTypeTable : MetadataTable<ExportedTypeRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteUInt32((uint)rows[i].Col1);
                buffer.WriteUInt32(rows[i].Col2);
                buffer.WriteString(rows[i].Col3);
                buffer.WriteString(rows[i].Col4);
                buffer.WriteCodedRID(rows[i].Col5, CodedIndex.Implementation);
            }
        }
    }

    internal sealed class ManifestResourceTable : MetadataTable<ManifestResourceRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteUInt32(rows[i].Col1);
                buffer.WriteUInt32((uint)rows[i].Col2);
                buffer.WriteString(rows[i].Col3);
                buffer.WriteCodedRID(rows[i].Col4, CodedIndex.Implementation);
            }
        }
    }

    internal sealed class NestedClassTable : SortedTable<NestedClassRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteRID(rows[i].Col1, Table.TypeDef); // NestedClass
                buffer.WriteRID(rows[i].Col2, Table.TypeDef); // EnclosingClass
            }
        }

        public override int Compare(NestedClassRow x, NestedClassRow y)
        {
            return Compare(x.Col1, y.Col1);
        }
    }

    internal sealed class GenericParamTable : MetadataTable<GenericParamRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteUInt16(rows[i].Col1); // Number
                buffer.WriteUInt16((ushort)rows[i].Col2); // Flags
                buffer.WriteCodedRID(rows[i].Col3, CodedIndex.TypeOrMethodDef); // Owner
                buffer.WriteString(rows[i].Col4); // Name
            }
        }
    }

    internal sealed class MethodSpecTable : MetadataTable<MethodSpecRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteCodedRID(rows[i].Col1, CodedIndex.MethodDefOrRef); // Method
                buffer.WriteBlob(rows[i].Col2); // Instantiation
            }
        }
    }

    internal sealed class GenericParamConstraintTable : MetadataTable<GenericParamConstraintRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteRID(rows[i].Col1, Table.GenericParam); // Owner
                buffer.WriteCodedRID(rows[i].Col2, CodedIndex.TypeDefOrRef); // Constraint
            }
        }
    }

    internal sealed class DocumentTable : MetadataTable<DocumentRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteBlob(rows[i].Col1); // Name
                buffer.WriteGuid(rows[i].Col2); // HashAlgorithm
                buffer.WriteBlob(rows[i].Col3); // Hash
                buffer.WriteGuid(rows[i].Col4); // Language
            }
        }
    }

    internal sealed class MethodDebugInformationTable : MetadataTable<MethodDebugInformationRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteRID(rows[i].Col1, Table.Document); // Document
                buffer.WriteBlob(rows[i].Col2); // SequencePoints
            }
        }
    }

    internal sealed class LocalScopeTable : MetadataTable<LocalScopeRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteRID(rows[i].Col1, Table.Method); // Method
                buffer.WriteRID(rows[i].Col2, Table.ImportScope); // ImportScope
                buffer.WriteRID(rows[i].Col3, Table.LocalVariable); // VariableList
                buffer.WriteRID(rows[i].Col4, Table.LocalConstant); // ConstantList
                buffer.WriteUInt32(rows[i].Col5); // StartOffset
                buffer.WriteUInt32(rows[i].Col6); // Length
            }
        }
    }

    internal sealed class LocalVariableTable : MetadataTable<LocalVariableRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteUInt16((ushort)rows[i].Col1); // Attributes
                buffer.WriteUInt16(rows[i].Col2); // Index
                buffer.WriteString(rows[i].Col3); // Name
            }
        }
    }

    internal sealed class LocalConstantTable : MetadataTable<LocalConstantRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteString(rows[i].Col1); // Name
                buffer.WriteBlob(rows[i].Col2); // Signature
            }
        }
    }

    internal sealed class ImportScopeTable : MetadataTable<ImportScopeRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteRID(rows[i].Col1, Table.ImportScope); // Parent
                buffer.WriteBlob(rows[i].Col2); // Imports
            }
        }
    }

    internal sealed class StateMachineMethodTable : MetadataTable<StateMachineMethodRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteRID(rows[i].Col1, Table.Method); // MoveNextMethod
                buffer.WriteRID(rows[i].Col2, Table.Method); // KickoffMethod
            }
        }
    }

    internal sealed class CustomDebugInformationTable : SortedTable<CustomDebugInformationRow>
    {
        public override void Write(TableHeapBuffer buffer)
        {
            for (int i = 0; i < length; i++)
            {
                buffer.WriteCodedRID(rows[i].Col1, CodedIndex.HasCustomDebugInformation); // Parent
                buffer.WriteGuid(rows[i].Col2); // Kind
                buffer.WriteBlob(rows[i].Col3); // Value
            }
        }

        public override int Compare(CustomDebugInformationRow x, CustomDebugInformationRow y)
        {
            return Compare(x.Col1, y.Col1);
        }
    }

    internal sealed class MetadataBuilder
    {
        internal readonly ModuleDefinition module;
        internal readonly ISymbolWriterProvider symbol_writer_provider;
        internal ISymbolWriter symbol_writer;
        internal readonly TextMap text_map;
        internal readonly string fq_name;
        internal readonly uint timestamp;
        private readonly Dictionary<TypeRefRow, MetadataToken> type_ref_map;
        private readonly Dictionary<uint, MetadataToken> type_spec_map;
        private readonly Dictionary<MemberRefRow, MetadataToken> member_ref_map;
        private readonly Dictionary<MethodSpecRow, MetadataToken> method_spec_map;
        private readonly Collection<GenericParameter> generic_parameters;
        internal readonly CodeWriter code;
        internal readonly DataBuffer data;
        internal readonly ResourceBuffer resources;
        internal readonly StringHeapBuffer string_heap;
        internal readonly GuidHeapBuffer guid_heap;
        internal readonly UserStringHeapBuffer user_string_heap;
        internal readonly BlobHeapBuffer blob_heap;
        internal readonly TableHeapBuffer table_heap;
        internal readonly PdbHeapBuffer pdb_heap;
        internal MetadataToken entry_point;
        internal RID type_rid = 1;
        internal RID field_rid = 1;
        internal RID method_rid = 1;
        internal RID param_rid = 1;
        internal RID property_rid = 1;
        internal RID event_rid = 1;
        internal RID local_variable_rid = 1;
        internal RID local_constant_rid = 1;
        private readonly TypeRefTable type_ref_table;
        private readonly TypeDefTable type_def_table;
        private readonly FieldTable field_table;
        private readonly MethodTable method_table;
        private readonly ParamTable param_table;
        private readonly InterfaceImplTable iface_impl_table;
        private readonly MemberRefTable member_ref_table;
        private readonly ConstantTable constant_table;
        private readonly CustomAttributeTable custom_attribute_table;
        private readonly DeclSecurityTable declsec_table;
        private readonly StandAloneSigTable standalone_sig_table;
        private readonly EventMapTable event_map_table;
        private readonly EventTable event_table;
        private readonly PropertyMapTable property_map_table;
        private readonly PropertyTable property_table;
        private readonly TypeSpecTable typespec_table;
        private readonly MethodSpecTable method_spec_table;
        internal MetadataBuilder metadata_builder;
        private readonly DocumentTable document_table;
        private readonly MethodDebugInformationTable method_debug_information_table;
        private readonly LocalScopeTable local_scope_table;
        private readonly LocalVariableTable local_variable_table;
        private readonly LocalConstantTable local_constant_table;
        private readonly ImportScopeTable import_scope_table;
        private readonly StateMachineMethodTable state_machine_method_table;
        private readonly CustomDebugInformationTable custom_debug_information_table;
        private readonly Dictionary<ImportScopeRow, MetadataToken> import_scope_map;
        private readonly Dictionary<string, MetadataToken> document_map;

        public MetadataBuilder(ModuleDefinition module, string fq_name, uint timestamp, ISymbolWriterProvider symbol_writer_provider)
        {
            this.module = module;
            text_map = CreateTextMap();
            this.fq_name = fq_name;
            this.timestamp = timestamp;
            this.symbol_writer_provider = symbol_writer_provider;

            code = new(this);
            data = new();
            resources = new();
            string_heap = new();
            guid_heap = new();
            user_string_heap = new();
            blob_heap = new();
            table_heap = new(module, this);

            type_ref_table = GetTable<TypeRefTable>(Table.TypeRef);
            type_def_table = GetTable<TypeDefTable>(Table.TypeDef);
            field_table = GetTable<FieldTable>(Table.Field);
            method_table = GetTable<MethodTable>(Table.Method);
            param_table = GetTable<ParamTable>(Table.Param);
            iface_impl_table = GetTable<InterfaceImplTable>(Table.InterfaceImpl);
            member_ref_table = GetTable<MemberRefTable>(Table.MemberRef);
            constant_table = GetTable<ConstantTable>(Table.Constant);
            custom_attribute_table = GetTable<CustomAttributeTable>(Table.CustomAttribute);
            declsec_table = GetTable<DeclSecurityTable>(Table.DeclSecurity);
            standalone_sig_table = GetTable<StandAloneSigTable>(Table.StandAloneSig);
            event_map_table = GetTable<EventMapTable>(Table.EventMap);
            event_table = GetTable<EventTable>(Table.Event);
            property_map_table = GetTable<PropertyMapTable>(Table.PropertyMap);
            property_table = GetTable<PropertyTable>(Table.Property);
            typespec_table = GetTable<TypeSpecTable>(Table.TypeSpec);
            method_spec_table = GetTable<MethodSpecTable>(Table.MethodSpec);

            RowEqualityComparer row_equality_comparer = new();
            type_ref_map = new(row_equality_comparer);
            type_spec_map = new();
            member_ref_map = new(row_equality_comparer);
            method_spec_map = new(row_equality_comparer);
            generic_parameters = new();

            document_table = GetTable<DocumentTable>(Table.Document);
            method_debug_information_table = GetTable<MethodDebugInformationTable>(Table.MethodDebugInformation);
            local_scope_table = GetTable<LocalScopeTable>(Table.LocalScope);
            local_variable_table = GetTable<LocalVariableTable>(Table.LocalVariable);
            local_constant_table = GetTable<LocalConstantTable>(Table.LocalConstant);
            import_scope_table = GetTable<ImportScopeTable>(Table.ImportScope);
            state_machine_method_table = GetTable<StateMachineMethodTable>(Table.StateMachineMethod);
            custom_debug_information_table = GetTable<CustomDebugInformationTable>(Table.CustomDebugInformation);

            document_map = new(StringComparer.Ordinal);
            import_scope_map = new(row_equality_comparer);
        }

        public MetadataBuilder(ModuleDefinition module, PortablePdbWriterProvider writer_provider)
        {
            this.module = module;
            text_map = new();
            symbol_writer_provider = writer_provider;

            string_heap = new();
            guid_heap = new();
            user_string_heap = new();
            blob_heap = new();
            table_heap = new(module, this);
            pdb_heap = new();

            document_table = GetTable<DocumentTable>(Table.Document);
            method_debug_information_table = GetTable<MethodDebugInformationTable>(Table.MethodDebugInformation);
            local_scope_table = GetTable<LocalScopeTable>(Table.LocalScope);
            local_variable_table = GetTable<LocalVariableTable>(Table.LocalVariable);
            local_constant_table = GetTable<LocalConstantTable>(Table.LocalConstant);
            import_scope_table = GetTable<ImportScopeTable>(Table.ImportScope);
            state_machine_method_table = GetTable<StateMachineMethodTable>(Table.StateMachineMethod);
            custom_debug_information_table = GetTable<CustomDebugInformationTable>(Table.CustomDebugInformation);

            RowEqualityComparer row_equality_comparer = new();

            document_map = new();
            import_scope_map = new(row_equality_comparer);
        }

        public void SetSymbolWriter(ISymbolWriter writer)
        {
            symbol_writer = writer;

            if (symbol_writer == null && module.HasImage && module.Image.HasDebugTables())
                symbol_writer = new PortablePdbWriter(this, module);
        }

        private TextMap CreateTextMap()
        {
            TextMap map = new();
            map.AddMap(TextSegment.ImportAddressTable, module.Architecture == TargetArchitecture.I386 ? 8 : 0);
            map.AddMap(TextSegment.CLIHeader, 0x48, 8);
            return map;
        }

        private TTable GetTable<TTable>(Table table) where TTable : MetadataTable, new()
        {
            return table_heap.GetTable<TTable>(table);
        }

        private uint GetStringIndex(string @string)
        {
            if (string.IsNullOrEmpty(@string))
                return 0;

            return string_heap.GetStringIndex(@string);
        }

        private uint GetGuidIndex(Guid guid)
        {
            return guid_heap.GetGuidIndex(guid);
        }

        private uint GetBlobIndex(ByteBuffer blob)
        {
            if (blob.length == 0)
                return 0;

            return blob_heap.GetBlobIndex(blob);
        }

        private uint GetBlobIndex(byte[] blob)
        {
            if (blob.IsNullOrEmpty())
                return 0;

            return GetBlobIndex(new ByteBuffer(blob));
        }

        public void BuildMetadata()
        {
            BuildModule();

            table_heap.string_offsets = string_heap.WriteStrings();
            table_heap.ComputeTableInformations();
            table_heap.WriteTableHeap();
        }

        private void BuildModule()
        {
            ModuleTable table = GetTable<ModuleTable>(Table.Module);
            table.row.Col1 = GetStringIndex(module.Name);
            table.row.Col2 = GetGuidIndex(module.Mvid);

            AssemblyDefinition assembly = module.Assembly;

            if (module.kind != ModuleKind.NetModule && assembly != null)
                BuildAssembly();

            if (module.HasAssemblyReferences)
                AddAssemblyReferences();

            if (module.HasModuleReferences)
                AddModuleReferences();

            if (module.HasResources)
                AddResources();

            if (module.HasExportedTypes)
                AddExportedTypes();

            BuildTypes();

            if (module.kind != ModuleKind.NetModule && assembly != null)
            {
                if (assembly.HasCustomAttributes)
                    AddCustomAttributes(assembly);

                if (assembly.HasSecurityDeclarations)
                    AddSecurityDeclarations(assembly);
            }

            if (module.HasCustomAttributes)
                AddCustomAttributes(module);

            if (module.EntryPoint != null)
                entry_point = LookupToken(module.EntryPoint);
        }

        private void BuildAssembly()
        {
            AssemblyDefinition assembly = module.Assembly;
            AssemblyNameDefinition name = assembly.Name;

            AssemblyTable table = GetTable<AssemblyTable>(Table.Assembly);

            table.row = new(name.HashAlgorithm, (ushort)name.Version.Major, (ushort)name.Version.Minor, (ushort)name.Version.Build, (ushort)name.Version.Revision, name.Attributes, GetBlobIndex(name.PublicKey), GetStringIndex(name.Name), GetStringIndex(name.Culture));

            if (assembly.Modules.Count > 1)
                BuildModules();
        }

        private void BuildModules()
        {
            Collection<ModuleDefinition> modules = this.module.Assembly.Modules;
            FileTable table = GetTable<FileTable>(Table.File);

            for (int i = 0; i < modules.Count; i++)
            {
                ModuleDefinition module = modules[i];
                if (module.IsMain)
                    continue;

#if NET_CORE
	            throw new NotSupportedException();
#else
                WriterParameters parameters = new()
                {
                    SymbolWriterProvider = symbol_writer_provider
                };

                string file_name = GetModuleFileName(module.Name);
                module.Write(file_name, parameters);

                byte[] hash = CryptoService.ComputeHash(file_name);

                table.AddRow(new(FileAttributes.ContainsMetaData, GetStringIndex(module.Name), GetBlobIndex(hash)));
#endif
            }
        }

#if !NET_CORE
        private string GetModuleFileName(string name)
        {
            if (string.IsNullOrEmpty(name))
                throw new NotSupportedException();

            string path = Path.GetDirectoryName(fq_name);
            return Path.Combine(path, name);
        }
#endif

        private void AddAssemblyReferences()
        {
            Collection<AssemblyNameReference> references = module.AssemblyReferences;
            AssemblyRefTable table = GetTable<AssemblyRefTable>(Table.AssemblyRef);

            if (module.IsWindowsMetadata())
                module.Projections.RemoveVirtualReferences(references);

            for (int i = 0; i < references.Count; i++)
            {
                AssemblyNameReference reference = references[i];

                byte[] key_or_token = reference.PublicKey.IsNullOrEmpty() ? reference.PublicKeyToken : reference.PublicKey;

                Version version = reference.Version;

                int rid = table.AddRow(new((ushort)version.Major, (ushort)version.Minor, (ushort)version.Build, (ushort)version.Revision, reference.Attributes, GetBlobIndex(key_or_token), GetStringIndex(reference.Name), GetStringIndex(reference.Culture), GetBlobIndex(reference.Hash)));

                reference.token = new(TokenType.AssemblyRef, rid);
            }

            if (module.IsWindowsMetadata())
                module.Projections.AddVirtualReferences(references);
        }

        private void AddModuleReferences()
        {
            Collection<ModuleReference> references = module.ModuleReferences;
            ModuleRefTable table = GetTable<ModuleRefTable>(Table.ModuleRef);

            for (int i = 0; i < references.Count; i++)
            {
                ModuleReference reference = references[i];

                reference.token = new(TokenType.ModuleRef, table.AddRow(GetStringIndex(reference.Name)));
            }
        }

        private void AddResources()
        {
            Collection<Resource> resources = module.Resources;
            ManifestResourceTable table = GetTable<ManifestResourceTable>(Table.ManifestResource);

            for (int i = 0; i < resources.Count; i++)
            {
                Resource resource = resources[i];

                ManifestResourceRow row = new(0, resource.Attributes, GetStringIndex(resource.Name), 0);

                switch (resource.ResourceType)
                {
                    case ResourceType.Embedded:
                        row.Col1 = AddEmbeddedResource((EmbeddedResource)resource);
                        break;
                    case ResourceType.Linked:
                        row.Col4 = CodedIndex.Implementation.CompressMetadataToken(new(TokenType.File, AddLinkedResource((LinkedResource)resource)));
                        break;
                    case ResourceType.AssemblyLinked:
                        row.Col4 = CodedIndex.Implementation.CompressMetadataToken(((AssemblyLinkedResource)resource).Assembly.MetadataToken);
                        break;
                    default:
                        throw new NotSupportedException();
                }

                table.AddRow(row);
            }
        }

        private uint AddLinkedResource(LinkedResource resource)
        {
            FileTable table = GetTable<FileTable>(Table.File);
            byte[] hash = resource.Hash;

            if (hash.IsNullOrEmpty())
                hash = CryptoService.ComputeHash(resource.File);

            return (uint)table.AddRow(new(FileAttributes.ContainsNoMetaData, GetStringIndex(resource.File), GetBlobIndex(hash)));
        }

        private uint AddEmbeddedResource(EmbeddedResource resource)
        {
            return resources.AddResource(resource.GetResourceData());
        }

        private void AddExportedTypes()
        {
            Collection<ExportedType> exported_types = module.ExportedTypes;
            ExportedTypeTable table = GetTable<ExportedTypeTable>(Table.ExportedType);

            for (int i = 0; i < exported_types.Count; i++)
            {
                ExportedType exported_type = exported_types[i];

                int rid = table.AddRow(new(exported_type.Attributes, (uint)exported_type.Identifier, GetStringIndex(exported_type.Name), GetStringIndex(exported_type.Namespace), MakeCodedRID(GetExportedTypeScope(exported_type), CodedIndex.Implementation)));

                exported_type.token = new(TokenType.ExportedType, rid);
            }
        }

        private MetadataToken GetExportedTypeScope(ExportedType exported_type)
        {
            if (exported_type.DeclaringType != null)
                return exported_type.DeclaringType.MetadataToken;

            IMetadataScope scope = exported_type.Scope;
            switch (scope.MetadataToken.TokenType)
            {
                case TokenType.AssemblyRef:
                    return scope.MetadataToken;
                case TokenType.ModuleRef:
                    FileTable file_table = GetTable<FileTable>(Table.File);
                    for (int i = 0; i < file_table.length; i++)
                        if (file_table.rows[i].Col2 == GetStringIndex(scope.Name))
                            return new(TokenType.File, i + 1);

                    break;
            }

            throw new NotSupportedException();
        }

        private void BuildTypes()
        {
            if (!module.HasTypes)
                return;

            AttachTokens();
            AddTypes();
            AddGenericParameters();
        }

        private void AttachTokens()
        {
            Collection<TypeDefinition> types = module.Types;

            for (int i = 0; i < types.Count; i++)
                AttachTypeToken(types[i]);
        }

        private void AttachTypeToken(TypeDefinition type)
        {
            TypeDefinitionProjection treatment = WindowsRuntimeProjections.RemoveProjection(type);

            type.token = new(TokenType.TypeDef, type_rid++);
            type.fields_range.Start = field_rid;
            type.methods_range.Start = method_rid;

            if (type.HasFields)
                AttachFieldsToken(type);

            if (type.HasMethods)
                AttachMethodsToken(type);

            if (type.HasNestedTypes)
                AttachNestedTypesToken(type);

            WindowsRuntimeProjections.ApplyProjection(type, treatment);
        }

        private void AttachNestedTypesToken(TypeDefinition type)
        {
            Collection<TypeDefinition> nested_types = type.NestedTypes;
            for (int i = 0; i < nested_types.Count; i++)
                AttachTypeToken(nested_types[i]);
        }

        private void AttachFieldsToken(TypeDefinition type)
        {
            Collection<FieldDefinition> fields = type.Fields;
            type.fields_range.Length = (uint)fields.Count;
            for (int i = 0; i < fields.Count; i++)
                fields[i].token = new(TokenType.Field, field_rid++);
        }

        private void AttachMethodsToken(TypeDefinition type)
        {
            Collection<MethodDefinition> methods = type.Methods;
            type.methods_range.Length = (uint)methods.Count;
            for (int i = 0; i < methods.Count; i++)
                methods[i].token = new(TokenType.Method, method_rid++);
        }

        private MetadataToken GetTypeToken(TypeReference type)
        {
            if (type == null)
                return MetadataToken.Zero;

            if (type.IsDefinition)
                return type.token;

            if (type.IsTypeSpecification())
                return GetTypeSpecToken(type);

            return GetTypeRefToken(type);
        }

        private MetadataToken GetTypeSpecToken(TypeReference type)
        {
            uint row = GetBlobIndex(GetTypeSpecSignature(type));

            MetadataToken token;
            if (type_spec_map.TryGetValue(row, out token))
                return token;

            return AddTypeSpecification(type, row);
        }

        private MetadataToken AddTypeSpecification(TypeReference type, uint row)
        {
            type.token = new(TokenType.TypeSpec, typespec_table.AddRow(row));

            MetadataToken token = type.token;
            type_spec_map.Add(row, token);
            return token;
        }

        private MetadataToken GetTypeRefToken(TypeReference type)
        {
            TypeReferenceProjection projection = WindowsRuntimeProjections.RemoveProjection(type);

            TypeRefRow row = CreateTypeRefRow(type);

            MetadataToken token;
            if (!type_ref_map.TryGetValue(row, out token))
                token = AddTypeReference(type, row);

            WindowsRuntimeProjections.ApplyProjection(type, projection);

            return token;
        }

        private TypeRefRow CreateTypeRefRow(TypeReference type)
        {
            MetadataToken scope_token = GetScopeToken(type);

            return new(MakeCodedRID(scope_token, CodedIndex.ResolutionScope), GetStringIndex(type.Name), GetStringIndex(type.Namespace));
        }

        private MetadataToken GetScopeToken(TypeReference type)
        {
            if (type.IsNested)
                return GetTypeRefToken(type.DeclaringType);

            IMetadataScope scope = type.Scope;

            if (scope == null)
                return MetadataToken.Zero;

            return scope.MetadataToken;
        }

        private static CodedRID MakeCodedRID(IMetadataTokenProvider provider, CodedIndex index)
        {
            return MakeCodedRID(provider.MetadataToken, index);
        }

        private static CodedRID MakeCodedRID(MetadataToken token, CodedIndex index)
        {
            return index.CompressMetadataToken(token);
        }

        private MetadataToken AddTypeReference(TypeReference type, TypeRefRow row)
        {
            type.token = new(TokenType.TypeRef, type_ref_table.AddRow(row));

            MetadataToken token = type.token;
            type_ref_map.Add(row, token);
            return token;
        }

        private void AddTypes()
        {
            Collection<TypeDefinition> types = module.Types;

            for (int i = 0; i < types.Count; i++)
                AddType(types[i]);
        }

        private void AddType(TypeDefinition type)
        {
            TypeDefinitionProjection treatment = WindowsRuntimeProjections.RemoveProjection(type);

            type_def_table.AddRow(new(type.Attributes, GetStringIndex(type.Name), GetStringIndex(type.Namespace), MakeCodedRID(GetTypeToken(type.BaseType), CodedIndex.TypeDefOrRef), type.fields_range.Start, type.methods_range.Start));

            if (type.HasGenericParameters)
                AddGenericParameters(type);

            if (type.HasInterfaces)
                AddInterfaces(type);

            if (type.HasLayoutInfo)
                AddLayoutInfo(type);

            if (type.HasFields)
                AddFields(type);

            if (type.HasMethods)
                AddMethods(type);

            if (type.HasProperties)
                AddProperties(type);

            if (type.HasEvents)
                AddEvents(type);

            if (type.HasCustomAttributes)
                AddCustomAttributes(type);

            if (type.HasSecurityDeclarations)
                AddSecurityDeclarations(type);

            if (type.HasNestedTypes)
                AddNestedTypes(type);

            WindowsRuntimeProjections.ApplyProjection(type, treatment);
        }

        private void AddGenericParameters(IGenericParameterProvider owner)
        {
            Collection<GenericParameter> parameters = owner.GenericParameters;

            for (int i = 0; i < parameters.Count; i++)
                generic_parameters.Add(parameters[i]);
        }

        private sealed class GenericParameterComparer : IComparer<GenericParameter>
        {
            public int Compare(GenericParameter a, GenericParameter b)
            {
                CodedRID a_owner = MakeCodedRID(a.Owner, CodedIndex.TypeOrMethodDef);
                CodedRID b_owner = MakeCodedRID(b.Owner, CodedIndex.TypeOrMethodDef);
                if (a_owner == b_owner)
                {
                    int a_pos = a.Position;
                    int b_pos = b.Position;
                    return a_pos == b_pos ? 0 : a_pos > b_pos ? 1 : -1;
                }

                return a_owner > b_owner ? 1 : -1;
            }
        }

        private void AddGenericParameters()
        {
            GenericParameter[] items = generic_parameters.items;
            int size = generic_parameters.size;
            Array.Sort(items, 0, size, new GenericParameterComparer());

            GenericParamTable generic_param_table = GetTable<GenericParamTable>(Table.GenericParam);
            GenericParamConstraintTable generic_param_constraint_table = GetTable<GenericParamConstraintTable>(Table.GenericParamConstraint);

            for (int i = 0; i < size; i++)
            {
                GenericParameter generic_parameter = items[i];

                int rid = generic_param_table.AddRow(new((ushort)generic_parameter.Position, generic_parameter.Attributes, MakeCodedRID(generic_parameter.Owner, CodedIndex.TypeOrMethodDef), GetStringIndex(generic_parameter.Name)));

                generic_parameter.token = new(TokenType.GenericParam, rid);

                if (generic_parameter.HasConstraints)
                    AddConstraints(generic_parameter, generic_param_constraint_table);

                if (generic_parameter.HasCustomAttributes)
                    AddCustomAttributes(generic_parameter);
            }
        }

        private void AddConstraints(GenericParameter generic_parameter, GenericParamConstraintTable table)
        {
            Collection<GenericParameterConstraint> constraints = generic_parameter.Constraints;

            uint gp_rid = generic_parameter.token.RID;

            for (int i = 0; i < constraints.Count; i++)
            {
                GenericParameterConstraint constraint = constraints[i];

                int rid = table.AddRow(new(gp_rid, MakeCodedRID(GetTypeToken(constraint.ConstraintType), CodedIndex.TypeDefOrRef)));

                constraint.token = new(TokenType.GenericParamConstraint, rid);

                if (constraint.HasCustomAttributes)
                    AddCustomAttributes(constraint);
            }
        }

        private void AddInterfaces(TypeDefinition type)
        {
            Collection<InterfaceImplementation> interfaces = type.Interfaces;
            uint type_rid = type.token.RID;

            for (int i = 0; i < interfaces.Count; i++)
            {
                InterfaceImplementation iface_impl = interfaces[i];

                int rid = iface_impl_table.AddRow(new(type_rid, MakeCodedRID(GetTypeToken(iface_impl.InterfaceType), CodedIndex.TypeDefOrRef)));

                iface_impl.token = new(TokenType.InterfaceImpl, rid);

                if (iface_impl.HasCustomAttributes)
                    AddCustomAttributes(iface_impl);
            }
        }

        private void AddLayoutInfo(TypeDefinition type)
        {
            ClassLayoutTable table = GetTable<ClassLayoutTable>(Table.ClassLayout);

            table.AddRow(new((ushort)type.PackingSize, (uint)type.ClassSize, type.token.RID));
        }

        private void AddNestedTypes(TypeDefinition type)
        {
            Collection<TypeDefinition> nested_types = type.NestedTypes;
            NestedClassTable nested_table = GetTable<NestedClassTable>(Table.NestedClass);

            for (int i = 0; i < nested_types.Count; i++)
            {
                TypeDefinition nested = nested_types[i];
                AddType(nested);
                nested_table.AddRow(new(nested.token.RID, type.token.RID));
            }
        }

        private void AddFields(TypeDefinition type)
        {
            Collection<FieldDefinition> fields = type.Fields;

            for (int i = 0; i < fields.Count; i++)
                AddField(fields[i]);
        }

        private void AddField(FieldDefinition field)
        {
            FieldDefinitionProjection projection = WindowsRuntimeProjections.RemoveProjection(field);

            field_table.AddRow(new(field.Attributes, GetStringIndex(field.Name), GetBlobIndex(GetFieldSignature(field))));

            if (!field.InitialValue.IsNullOrEmpty())
                AddFieldRVA(field);

            if (field.HasLayoutInfo)
                AddFieldLayout(field);

            if (field.HasCustomAttributes)
                AddCustomAttributes(field);

            if (field.HasConstant)
                AddConstant(field, field.FieldType);

            if (field.HasMarshalInfo)
                AddMarshalInfo(field);

            WindowsRuntimeProjections.ApplyProjection(field, projection);
        }

        private void AddFieldRVA(FieldDefinition field)
        {
            FieldRVATable table = GetTable<FieldRVATable>(Table.FieldRVA);
            table.AddRow(new(data.AddData(field.InitialValue), field.token.RID));
        }

        private void AddFieldLayout(FieldDefinition field)
        {
            FieldLayoutTable table = GetTable<FieldLayoutTable>(Table.FieldLayout);
            table.AddRow(new((uint)field.Offset, field.token.RID));
        }

        private void AddMethods(TypeDefinition type)
        {
            Collection<MethodDefinition> methods = type.Methods;

            for (int i = 0; i < methods.Count; i++)
                AddMethod(methods[i]);
        }

        private void AddMethod(MethodDefinition method)
        {
            MethodDefinitionProjection projection = WindowsRuntimeProjections.RemoveProjection(method);

            method_table.AddRow(new(method.HasBody ? code.WriteMethodBody(method) : 0, method.ImplAttributes, method.Attributes, GetStringIndex(method.Name), GetBlobIndex(GetMethodSignature(method)), param_rid));

            AddParameters(method);

            if (method.HasGenericParameters)
                AddGenericParameters(method);

            if (method.IsPInvokeImpl)
                AddPInvokeInfo(method);

            if (method.HasCustomAttributes)
                AddCustomAttributes(method);

            if (method.HasSecurityDeclarations)
                AddSecurityDeclarations(method);

            if (method.HasOverrides)
                AddOverrides(method);

            WindowsRuntimeProjections.ApplyProjection(method, projection);
        }

        private void AddParameters(MethodDefinition method)
        {
            ParameterDefinition return_parameter = method.MethodReturnType.parameter;

            if (return_parameter != null && RequiresParameterRow(return_parameter))
                AddParameter(0, return_parameter, param_table);

            if (!method.HasParameters)
                return;

            Collection<ParameterDefinition> parameters = method.Parameters;

            for (int i = 0; i < parameters.Count; i++)
            {
                ParameterDefinition parameter = parameters[i];
                if (!RequiresParameterRow(parameter))
                    continue;

                AddParameter((ushort)(i + 1), parameter, param_table);
            }
        }

        private void AddPInvokeInfo(MethodDefinition method)
        {
            PInvokeInfo pinvoke = method.PInvokeInfo;
            if (pinvoke == null)
                return;

            ImplMapTable table = GetTable<ImplMapTable>(Table.ImplMap);
            table.AddRow(new(pinvoke.Attributes, MakeCodedRID(method, CodedIndex.MemberForwarded), GetStringIndex(pinvoke.EntryPoint), pinvoke.Module.MetadataToken.RID));
        }

        private void AddOverrides(MethodDefinition method)
        {
            Collection<MethodReference> overrides = method.Overrides;
            MethodImplTable table = GetTable<MethodImplTable>(Table.MethodImpl);

            for (int i = 0; i < overrides.Count; i++)
            {
                table.AddRow(new(method.DeclaringType.token.RID, MakeCodedRID(method, CodedIndex.MethodDefOrRef), MakeCodedRID(LookupToken(overrides[i]), CodedIndex.MethodDefOrRef)));
            }
        }

        private static bool RequiresParameterRow(ParameterDefinition parameter)
        {
            return !string.IsNullOrEmpty(parameter.Name) || parameter.Attributes != ParameterAttributes.None || parameter.HasMarshalInfo || parameter.HasConstant || parameter.HasCustomAttributes;
        }

        private void AddParameter(ushort sequence, ParameterDefinition parameter, ParamTable table)
        {
            table.AddRow(new(parameter.Attributes, sequence, GetStringIndex(parameter.Name)));

            parameter.token = new(TokenType.Param, param_rid++);

            if (parameter.HasCustomAttributes)
                AddCustomAttributes(parameter);

            if (parameter.HasConstant)
                AddConstant(parameter, parameter.ParameterType);

            if (parameter.HasMarshalInfo)
                AddMarshalInfo(parameter);
        }

        private void AddMarshalInfo(IMarshalInfoProvider owner)
        {
            FieldMarshalTable table = GetTable<FieldMarshalTable>(Table.FieldMarshal);

            table.AddRow(new(MakeCodedRID(owner, CodedIndex.HasFieldMarshal), GetBlobIndex(GetMarshalInfoSignature(owner))));
        }

        private void AddProperties(TypeDefinition type)
        {
            Collection<PropertyDefinition> properties = type.Properties;

            property_map_table.AddRow(new(type.token.RID, property_rid));

            for (int i = 0; i < properties.Count; i++)
                AddProperty(properties[i]);
        }

        private void AddProperty(PropertyDefinition property)
        {
            property_table.AddRow(new(property.Attributes, GetStringIndex(property.Name), GetBlobIndex(GetPropertySignature(property))));
            property.token = new(TokenType.Property, property_rid++);

            MethodDefinition method = property.GetMethod;
            if (method != null)
                AddSemantic(MethodSemanticsAttributes.Getter, property, method);

            method = property.SetMethod;
            if (method != null)
                AddSemantic(MethodSemanticsAttributes.Setter, property, method);

            if (property.HasOtherMethods)
                AddOtherSemantic(property, property.OtherMethods);

            if (property.HasCustomAttributes)
                AddCustomAttributes(property);

            if (property.HasConstant)
                AddConstant(property, property.PropertyType);
        }

        private void AddOtherSemantic(IMetadataTokenProvider owner, Collection<MethodDefinition> others)
        {
            for (int i = 0; i < others.Count; i++)
                AddSemantic(MethodSemanticsAttributes.Other, owner, others[i]);
        }

        private void AddEvents(TypeDefinition type)
        {
            Collection<EventDefinition> events = type.Events;

            event_map_table.AddRow(new(type.token.RID, event_rid));

            for (int i = 0; i < events.Count; i++)
                AddEvent(events[i]);
        }

        private void AddEvent(EventDefinition @event)
        {
            event_table.AddRow(new(@event.Attributes, GetStringIndex(@event.Name), MakeCodedRID(GetTypeToken(@event.EventType), CodedIndex.TypeDefOrRef)));
            @event.token = new(TokenType.Event, event_rid++);

            MethodDefinition method = @event.AddMethod;
            if (method != null)
                AddSemantic(MethodSemanticsAttributes.AddOn, @event, method);

            method = @event.InvokeMethod;
            if (method != null)
                AddSemantic(MethodSemanticsAttributes.Fire, @event, method);

            method = @event.RemoveMethod;
            if (method != null)
                AddSemantic(MethodSemanticsAttributes.RemoveOn, @event, method);

            if (@event.HasOtherMethods)
                AddOtherSemantic(@event, @event.OtherMethods);

            if (@event.HasCustomAttributes)
                AddCustomAttributes(@event);
        }

        private void AddSemantic(MethodSemanticsAttributes semantics, IMetadataTokenProvider provider, MethodDefinition method)
        {
            method.SemanticsAttributes = semantics;
            MethodSemanticsTable table = GetTable<MethodSemanticsTable>(Table.MethodSemantics);

            table.AddRow(new(semantics, method.token.RID, MakeCodedRID(provider, CodedIndex.HasSemantics)));
        }

        private void AddConstant(IConstantProvider owner, TypeReference type)
        {
            object constant = owner.Constant;
            ElementType etype = GetConstantType(type, constant);

            constant_table.AddRow(new(etype, MakeCodedRID(owner.MetadataToken, CodedIndex.HasConstant), GetBlobIndex(GetConstantSignature(etype, constant))));
        }

        private static ElementType GetConstantType(TypeReference constant_type, object constant)
        {
            if (constant == null)
                return ElementType.Class;

            ElementType etype = constant_type.etype;
            switch (etype)
            {
                case ElementType.None:
                    TypeDefinition type = constant_type.CheckedResolve();
                    if (type.IsEnum)
                        return GetConstantType(type.GetEnumUnderlyingType(), constant);

                    return ElementType.Class;
                case ElementType.String:
                    return ElementType.String;
                case ElementType.Object:
                    return GetConstantType(constant.GetType());
                case ElementType.Array:
                case ElementType.SzArray:
                case ElementType.MVar:
                case ElementType.Var:
                    return ElementType.Class;
                case ElementType.GenericInst:
                    GenericInstanceType generic_instance = (GenericInstanceType)constant_type;
                    if (generic_instance.ElementType.IsTypeOf("System", "Nullable`1"))
                        return GetConstantType(generic_instance.GenericArguments[0], constant);

                    return GetConstantType(((TypeSpecification)constant_type).ElementType, constant);
                case ElementType.CModOpt:
                case ElementType.CModReqD:
                case ElementType.ByRef:
                case ElementType.Sentinel:
                    return GetConstantType(((TypeSpecification)constant_type).ElementType, constant);
                case ElementType.Boolean:
                case ElementType.Char:
                case ElementType.I:
                case ElementType.I1:
                case ElementType.I2:
                case ElementType.I4:
                case ElementType.I8:
                case ElementType.U:
                case ElementType.U1:
                case ElementType.U2:
                case ElementType.U4:
                case ElementType.U8:
                case ElementType.R4:
                case ElementType.R8:
                    return GetConstantType(constant.GetType());
                default:
                    return etype;
            }
        }

        private static ElementType GetConstantType(Type type)
        {
            switch (Type.GetTypeCode(type))
            {
                case TypeCode.Boolean:
                    return ElementType.Boolean;
                case TypeCode.Byte:
                    return ElementType.U1;
                case TypeCode.SByte:
                    return ElementType.I1;
                case TypeCode.Char:
                    return ElementType.Char;
                case TypeCode.Int16:
                    return ElementType.I2;
                case TypeCode.UInt16:
                    return ElementType.U2;
                case TypeCode.Int32:
                    return ElementType.I4;
                case TypeCode.UInt32:
                    return ElementType.U4;
                case TypeCode.Int64:
                    return ElementType.I8;
                case TypeCode.UInt64:
                    return ElementType.U8;
                case TypeCode.Single:
                    return ElementType.R4;
                case TypeCode.Double:
                    return ElementType.R8;
                case TypeCode.String:
                    return ElementType.String;
                default:
                    throw new NotSupportedException(type.FullName);
            }
        }

        private void AddCustomAttributes(ICustomAttributeProvider owner)
        {
            Collection<CustomAttribute> custom_attributes = owner.CustomAttributes;

            for (int i = 0; i < custom_attributes.Count; i++)
            {
                CustomAttribute attribute = custom_attributes[i];

                CustomAttributeValueProjection projection = WindowsRuntimeProjections.RemoveProjection(attribute);

                custom_attribute_table.AddRow(new(MakeCodedRID(owner, CodedIndex.HasCustomAttribute), MakeCodedRID(LookupToken(attribute.Constructor), CodedIndex.CustomAttributeType), GetBlobIndex(GetCustomAttributeSignature(attribute))));

                WindowsRuntimeProjections.ApplyProjection(attribute, projection);
            }
        }

        private void AddSecurityDeclarations(ISecurityDeclarationProvider owner)
        {
            Collection<SecurityDeclaration> declarations = owner.SecurityDeclarations;

            for (int i = 0; i < declarations.Count; i++)
            {
                SecurityDeclaration declaration = declarations[i];

                declsec_table.AddRow(new(declaration.Action, MakeCodedRID(owner, CodedIndex.HasDeclSecurity), GetBlobIndex(GetSecurityDeclarationSignature(declaration))));
            }
        }

        private MetadataToken GetMemberRefToken(MemberReference member)
        {
            MemberRefRow row = CreateMemberRefRow(member);

            MetadataToken token;
            if (!member_ref_map.TryGetValue(row, out token))
                token = AddMemberReference(member, row);

            return token;
        }

        private MemberRefRow CreateMemberRefRow(MemberReference member)
        {
            return new(MakeCodedRID(GetTypeToken(member.DeclaringType), CodedIndex.MemberRefParent), GetStringIndex(member.Name), GetBlobIndex(GetMemberRefSignature(member)));
        }

        private MetadataToken AddMemberReference(MemberReference member, MemberRefRow row)
        {
            member.token = new(TokenType.MemberRef, member_ref_table.AddRow(row));

            MetadataToken token = member.token;
            member_ref_map.Add(row, token);
            return token;
        }

        private MetadataToken GetMethodSpecToken(MethodSpecification method_spec)
        {
            MethodSpecRow row = CreateMethodSpecRow(method_spec);

            MetadataToken token;
            if (method_spec_map.TryGetValue(row, out token))
                return token;

            AddMethodSpecification(method_spec, row);

            return method_spec.token;
        }

        private void AddMethodSpecification(MethodSpecification method_spec, MethodSpecRow row)
        {
            method_spec.token = new(TokenType.MethodSpec, method_spec_table.AddRow(row));
            method_spec_map.Add(row, method_spec.token);
        }

        private MethodSpecRow CreateMethodSpecRow(MethodSpecification method_spec)
        {
            return new(MakeCodedRID(LookupToken(method_spec.ElementMethod), CodedIndex.MethodDefOrRef), GetBlobIndex(GetMethodSpecSignature(method_spec)));
        }

        private SignatureWriter CreateSignatureWriter()
        {
            return new(this);
        }

        private SignatureWriter GetMethodSpecSignature(MethodSpecification method_spec)
        {
            if (!method_spec.IsGenericInstance)
                throw new NotSupportedException();

            GenericInstanceMethod generic_instance = (GenericInstanceMethod)method_spec;

            SignatureWriter signature = CreateSignatureWriter();
            signature.WriteByte(0x0a);

            signature.WriteGenericInstanceSignature(generic_instance);

            return signature;
        }

        public uint AddStandAloneSignature(uint signature)
        {
            return (uint)standalone_sig_table.AddRow(signature);
        }

        public uint GetLocalVariableBlobIndex(Collection<VariableDefinition> variables)
        {
            return GetBlobIndex(GetVariablesSignature(variables));
        }

        public uint GetCallSiteBlobIndex(CallSite call_site)
        {
            return GetBlobIndex(GetMethodSignature(call_site));
        }

        public uint GetConstantTypeBlobIndex(TypeReference constant_type)
        {
            return GetBlobIndex(GetConstantTypeSignature(constant_type));
        }

        private SignatureWriter GetVariablesSignature(Collection<VariableDefinition> variables)
        {
            SignatureWriter signature = CreateSignatureWriter();
            signature.WriteByte(0x7);
            signature.WriteCompressedUInt32((uint)variables.Count);
            for (int i = 0; i < variables.Count; i++)
                signature.WriteTypeSignature(variables[i].VariableType);
            return signature;
        }

        private SignatureWriter GetConstantTypeSignature(TypeReference constant_type)
        {
            SignatureWriter signature = CreateSignatureWriter();
            signature.WriteByte(0x6);
            signature.WriteTypeSignature(constant_type);
            return signature;
        }

        private SignatureWriter GetFieldSignature(FieldReference field)
        {
            SignatureWriter signature = CreateSignatureWriter();
            signature.WriteByte(0x6);
            signature.WriteTypeSignature(field.FieldType);
            return signature;
        }

        private SignatureWriter GetMethodSignature(IMethodSignature method)
        {
            SignatureWriter signature = CreateSignatureWriter();
            signature.WriteMethodSignature(method);
            return signature;
        }

        private SignatureWriter GetMemberRefSignature(MemberReference member)
        {
            FieldReference field = member as FieldReference;
            if (field != null)
                return GetFieldSignature(field);

            MethodReference method = member as MethodReference;
            if (method != null)
                return GetMethodSignature(method);

            throw new NotSupportedException();
        }

        private SignatureWriter GetPropertySignature(PropertyDefinition property)
        {
            SignatureWriter signature = CreateSignatureWriter();
            byte calling_convention = 0x8;
            if (property.HasThis)
                calling_convention |= 0x20;

            uint param_count = 0;
            Collection<ParameterDefinition> parameters = null;

            if (property.HasParameters)
            {
                parameters = property.Parameters;
                param_count = (uint)parameters.Count;
            }

            signature.WriteByte(calling_convention);
            signature.WriteCompressedUInt32(param_count);
            signature.WriteTypeSignature(property.PropertyType);

            if (param_count == 0)
                return signature;

            for (int i = 0; i < param_count; i++)
                signature.WriteTypeSignature(parameters[i].ParameterType);

            return signature;
        }

        private SignatureWriter GetTypeSpecSignature(TypeReference type)
        {
            SignatureWriter signature = CreateSignatureWriter();
            signature.WriteTypeSignature(type);
            return signature;
        }

        private SignatureWriter GetConstantSignature(ElementType type, object value)
        {
            SignatureWriter signature = CreateSignatureWriter();

            switch (type)
            {
                case ElementType.Array:
                case ElementType.SzArray:
                case ElementType.Class:
                case ElementType.Object:
                case ElementType.None:
                case ElementType.Var:
                case ElementType.MVar:
                    signature.WriteInt32(0);
                    break;
                case ElementType.String:
                    signature.WriteConstantString((string)value);
                    break;
                default:
                    signature.WriteConstantPrimitive(value);
                    break;
            }

            return signature;
        }

        private SignatureWriter GetCustomAttributeSignature(CustomAttribute attribute)
        {
            SignatureWriter signature = CreateSignatureWriter();
            if (!attribute.resolved)
            {
                signature.WriteBytes(attribute.GetBlob());
                return signature;
            }

            signature.WriteUInt16(0x0001);

            signature.WriteCustomAttributeConstructorArguments(attribute);

            signature.WriteCustomAttributeNamedArguments(attribute);

            return signature;
        }

        private SignatureWriter GetSecurityDeclarationSignature(SecurityDeclaration declaration)
        {
            SignatureWriter signature = CreateSignatureWriter();

            if (!declaration.resolved)
                signature.WriteBytes(declaration.GetBlob());
            else if (module.Runtime < TargetRuntime.Net_2_0)
                signature.WriteXmlSecurityDeclaration(declaration);
            else
                signature.WriteSecurityDeclaration(declaration);

            return signature;
        }

        private SignatureWriter GetMarshalInfoSignature(IMarshalInfoProvider owner)
        {
            SignatureWriter signature = CreateSignatureWriter();

            signature.WriteMarshalInfo(owner.MarshalInfo);

            return signature;
        }

        private static Exception CreateForeignMemberException(MemberReference member)
        {
            return new ArgumentException(string.Format("Member '{0}' is declared in another module and needs to be imported", member));
        }

        public MetadataToken LookupToken(IMetadataTokenProvider provider)
        {
            if (provider == null)
                throw new ArgumentNullException();

            if (metadata_builder != null)
                return metadata_builder.LookupToken(provider);

            MemberReference member = provider as MemberReference;
            if (member == null || member.Module != module)
                throw CreateForeignMemberException(member);

            MetadataToken token = provider.MetadataToken;

            switch (token.TokenType)
            {
                case TokenType.TypeDef:
                case TokenType.Method:
                case TokenType.Field:
                case TokenType.Event:
                case TokenType.Property:
                    return token;
                case TokenType.TypeRef:
                case TokenType.TypeSpec:
                case TokenType.GenericParam:
                    return GetTypeToken((TypeReference)provider);
                case TokenType.MethodSpec:
                    return GetMethodSpecToken((MethodSpecification)provider);
                case TokenType.MemberRef:
                    return GetMemberRefToken(member);
                default:
                    throw new NotSupportedException();
            }
        }

        public void AddMethodDebugInformation(MethodDebugInformation method_info)
        {
            if (method_info.HasSequencePoints)
                AddSequencePoints(method_info);

            if (method_info.Scope != null)
                AddLocalScope(method_info, method_info.Scope);

            if (method_info.StateMachineKickOffMethod != null)
                AddStateMachineMethod(method_info);

            AddCustomDebugInformations(method_info.Method);
        }

        private void AddStateMachineMethod(MethodDebugInformation method_info)
        {
            state_machine_method_table.AddRow(new(method_info.Method.MetadataToken.RID, method_info.StateMachineKickOffMethod.MetadataToken.RID));
        }

        private void AddLocalScope(MethodDebugInformation method_info, ScopeDebugInformation scope)
        {
            int rid = local_scope_table.AddRow(new(method_info.Method.MetadataToken.RID, scope.import != null ? AddImportScope(scope.import) : 0, local_variable_rid, local_constant_rid, (uint)scope.Start.Offset, (uint)((scope.End.IsEndOfMethod ? method_info.code_size : scope.End.Offset) - scope.Start.Offset)));

            scope.token = new(TokenType.LocalScope, rid);

            AddCustomDebugInformations(scope);

            if (scope.HasVariables)
                AddLocalVariables(scope);

            if (scope.HasConstants)
                AddLocalConstants(scope);

            for (int i = 0; i < scope.Scopes.Count; i++)
                AddLocalScope(method_info, scope.Scopes[i]);
        }

        private void AddLocalVariables(ScopeDebugInformation scope)
        {
            for (int i = 0; i < scope.Variables.Count; i++)
            {
                VariableDebugInformation variable = scope.Variables[i];
                local_variable_table.AddRow(new(variable.Attributes, (ushort)variable.Index, GetStringIndex(variable.Name)));
                variable.token = new(TokenType.LocalVariable, local_variable_rid);
                local_variable_rid++;

                AddCustomDebugInformations(variable);
            }
        }

        private void AddLocalConstants(ScopeDebugInformation scope)
        {
            for (int i = 0; i < scope.Constants.Count; i++)
            {
                ConstantDebugInformation constant = scope.Constants[i];
                local_constant_table.AddRow(new(GetStringIndex(constant.Name), GetBlobIndex(GetConstantSignature(constant))));
                constant.token = new(TokenType.LocalConstant, local_constant_rid);
                local_constant_rid++;
            }
        }

        private SignatureWriter GetConstantSignature(ConstantDebugInformation constant)
        {
            TypeReference type = constant.ConstantType;

            SignatureWriter signature = CreateSignatureWriter();
            signature.WriteTypeSignature(type);

            if (type.IsTypeOf("System", "Decimal"))
            {
                int[] bits = decimal.GetBits((decimal)constant.Value);

                uint low = (uint)bits[0];
                uint mid = (uint)bits[1];
                uint high = (uint)bits[2];

                byte scale = (byte)(bits[3] >> 16);
                bool negative = (bits[3] & 0x80000000) != 0;

                signature.WriteByte((byte)(scale | (negative ? 0x80 : 0x00)));
                signature.WriteUInt32(low);
                signature.WriteUInt32(mid);
                signature.WriteUInt32(high);

                return signature;
            }

            if (type.IsTypeOf("System", "DateTime"))
            {
                DateTime date = (DateTime)constant.Value;
                signature.WriteInt64(date.Ticks);
                return signature;
            }

            signature.WriteBytes(GetConstantSignature(type.etype, constant.Value));

            return signature;
        }

        public void AddCustomDebugInformations(ICustomDebugInformationProvider provider)
        {
            if (!provider.HasCustomDebugInformations)
                return;

            Collection<CustomDebugInformation> custom_infos = provider.CustomDebugInformations;

            for (int i = 0; i < custom_infos.Count; i++)
            {
                CustomDebugInformation custom_info = custom_infos[i];
                switch (custom_info.Kind)
                {
                    case CustomDebugInformationKind.Binary:
                        BinaryCustomDebugInformation binary_info = (BinaryCustomDebugInformation)custom_info;
                        AddCustomDebugInformation(provider, binary_info, GetBlobIndex(binary_info.Data));
                        break;
                    case CustomDebugInformationKind.AsyncMethodBody:
                        AddAsyncMethodBodyDebugInformation(provider, (AsyncMethodBodyDebugInformation)custom_info);
                        break;
                    case CustomDebugInformationKind.StateMachineScope:
                        AddStateMachineScopeDebugInformation(provider, (StateMachineScopeDebugInformation)custom_info);
                        break;
                    case CustomDebugInformationKind.EmbeddedSource:
                        AddEmbeddedSourceDebugInformation(provider, (EmbeddedSourceDebugInformation)custom_info);
                        break;
                    case CustomDebugInformationKind.SourceLink:
                        AddSourceLinkDebugInformation(provider, (SourceLinkDebugInformation)custom_info);
                        break;
                    default:
                        throw new NotImplementedException();
                }
            }
        }

        private void AddStateMachineScopeDebugInformation(ICustomDebugInformationProvider provider, StateMachineScopeDebugInformation state_machine_scope)
        {
            MethodDebugInformation method_info = ((MethodDefinition)provider).DebugInformation;

            SignatureWriter signature = CreateSignatureWriter();

            Collection<StateMachineScope> scopes = state_machine_scope.Scopes;

            for (int i = 0; i < scopes.Count; i++)
            {
                StateMachineScope scope = scopes[i];
                signature.WriteUInt32((uint)scope.Start.Offset);

                int end_offset = scope.End.IsEndOfMethod ? method_info.code_size : scope.End.Offset;

                signature.WriteUInt32((uint)(end_offset - scope.Start.Offset));
            }

            AddCustomDebugInformation(provider, state_machine_scope, signature);
        }

        private void AddAsyncMethodBodyDebugInformation(ICustomDebugInformationProvider provider, AsyncMethodBodyDebugInformation async_method)
        {
            SignatureWriter signature = CreateSignatureWriter();
            signature.WriteUInt32((uint)async_method.catch_handler.Offset + 1);

            if (!async_method.yields.IsNullOrEmpty())
            {
                for (int i = 0; i < async_method.yields.Count; i++)
                {
                    signature.WriteUInt32((uint)async_method.yields[i].Offset);
                    signature.WriteUInt32((uint)async_method.resumes[i].Offset);
                    signature.WriteCompressedUInt32(async_method.resume_methods[i].MetadataToken.RID);
                }
            }

            AddCustomDebugInformation(provider, async_method, signature);
        }

        private void AddEmbeddedSourceDebugInformation(ICustomDebugInformationProvider provider, EmbeddedSourceDebugInformation embedded_source)
        {
            SignatureWriter signature = CreateSignatureWriter();

            if (!embedded_source.resolved)
            {
                signature.WriteBytes(embedded_source.ReadRawEmbeddedSourceDebugInformation());
                AddCustomDebugInformation(provider, embedded_source, signature);
                return;
            }

            byte[] content = embedded_source.content ?? Empty<byte>.Array;
            if (embedded_source.compress)
            {
                signature.WriteInt32(content.Length);

                MemoryStream decompressed_stream = new(content);
                MemoryStream content_stream = new();

                using (DeflateStream compress_stream = new(content_stream, CompressionMode.Compress, leaveOpen: true))
                {
                    decompressed_stream.CopyTo(compress_stream);
                }

                signature.WriteBytes(content_stream.ToArray());
            }
            else
            {
                signature.WriteInt32(0);
                signature.WriteBytes(content);
            }

            AddCustomDebugInformation(provider, embedded_source, signature);
        }

        private void AddSourceLinkDebugInformation(ICustomDebugInformationProvider provider, SourceLinkDebugInformation source_link)
        {
            SignatureWriter signature = CreateSignatureWriter();
            signature.WriteBytes(Encoding.UTF8.GetBytes(source_link.content));

            AddCustomDebugInformation(provider, source_link, signature);
        }

        private void AddCustomDebugInformation(ICustomDebugInformationProvider provider, CustomDebugInformation custom_info, SignatureWriter signature)
        {
            AddCustomDebugInformation(provider, custom_info, GetBlobIndex(signature));
        }

        private void AddCustomDebugInformation(ICustomDebugInformationProvider provider, CustomDebugInformation custom_info, uint blob_index)
        {
            int rid = custom_debug_information_table.AddRow(new(MakeCodedRID(provider.MetadataToken, CodedIndex.HasCustomDebugInformation), GetGuidIndex(custom_info.Identifier), blob_index));

            custom_info.token = new(TokenType.CustomDebugInformation, rid);
        }

        private uint AddImportScope(ImportDebugInformation import)
        {
            uint parent = 0;
            if (import.Parent != null)
                parent = AddImportScope(import.Parent);

            uint targets_index = 0;
            if (import.HasTargets)
            {
                SignatureWriter signature = CreateSignatureWriter();

                for (int i = 0; i < import.Targets.Count; i++)
                    AddImportTarget(import.Targets[i], signature);

                targets_index = GetBlobIndex(signature);
            }

            ImportScopeRow row = new(parent, targets_index);

            MetadataToken import_token;
            if (import_scope_map.TryGetValue(row, out import_token))
                return import_token.RID;

            import_token = new(TokenType.ImportScope, import_scope_table.AddRow(row));
            import_scope_map.Add(row, import_token);

            return import_token.RID;
        }

        private void AddImportTarget(ImportTarget target, SignatureWriter signature)
        {
            signature.WriteCompressedUInt32((uint)target.kind);

            switch (target.kind)
            {
                case ImportTargetKind.ImportNamespace:
                    signature.WriteCompressedUInt32(GetUTF8StringBlobIndex(target.@namespace));
                    break;
                case ImportTargetKind.ImportNamespaceInAssembly:
                    signature.WriteCompressedUInt32(target.reference.MetadataToken.RID);
                    signature.WriteCompressedUInt32(GetUTF8StringBlobIndex(target.@namespace));
                    break;
                case ImportTargetKind.ImportType:
                    signature.WriteTypeToken(target.type);
                    break;
                case ImportTargetKind.ImportXmlNamespaceWithAlias:
                    signature.WriteCompressedUInt32(GetUTF8StringBlobIndex(target.alias));
                    signature.WriteCompressedUInt32(GetUTF8StringBlobIndex(target.@namespace));
                    break;
                case ImportTargetKind.ImportAlias:
                    signature.WriteCompressedUInt32(GetUTF8StringBlobIndex(target.alias));
                    break;
                case ImportTargetKind.DefineAssemblyAlias:
                    signature.WriteCompressedUInt32(GetUTF8StringBlobIndex(target.alias));
                    signature.WriteCompressedUInt32(target.reference.MetadataToken.RID);
                    break;
                case ImportTargetKind.DefineNamespaceAlias:
                    signature.WriteCompressedUInt32(GetUTF8StringBlobIndex(target.alias));
                    signature.WriteCompressedUInt32(GetUTF8StringBlobIndex(target.@namespace));
                    break;
                case ImportTargetKind.DefineNamespaceInAssemblyAlias:
                    signature.WriteCompressedUInt32(GetUTF8StringBlobIndex(target.alias));
                    signature.WriteCompressedUInt32(target.reference.MetadataToken.RID);
                    signature.WriteCompressedUInt32(GetUTF8StringBlobIndex(target.@namespace));
                    break;
                case ImportTargetKind.DefineTypeAlias:
                    signature.WriteCompressedUInt32(GetUTF8StringBlobIndex(target.alias));
                    signature.WriteTypeToken(target.type);
                    break;
            }
        }

        private uint GetUTF8StringBlobIndex(string s)
        {
            return GetBlobIndex(Encoding.UTF8.GetBytes(s));
        }

        public MetadataToken GetDocumentToken(Document document)
        {
            MetadataToken token;
            if (document_map.TryGetValue(document.Url, out token))
                return token;

            token = new(TokenType.Document, document_table.AddRow(new(GetBlobIndex(GetDocumentNameSignature(document)), GetGuidIndex(document.HashAlgorithm.ToGuid()), GetBlobIndex(document.Hash), GetGuidIndex(document.Language.ToGuid()))));

            document.token = token;

            AddCustomDebugInformations(document);

            document_map.Add(document.Url, token);

            return token;
        }

        private SignatureWriter GetDocumentNameSignature(Document document)
        {
            string name = document.Url;
            SignatureWriter signature = CreateSignatureWriter();

            char separator;
            if (!TryGetDocumentNameSeparator(name, out separator))
            {
                signature.WriteByte(0);
                signature.WriteCompressedUInt32(GetUTF8StringBlobIndex(name));
                return signature;
            }

            signature.WriteByte((byte)separator);
            string[] parts = name.Split(new[] { separator });
            for (int i = 0; i < parts.Length; i++)
            {
                if (parts[i] == String.Empty)
                    signature.WriteCompressedUInt32(0);
                else
                    signature.WriteCompressedUInt32(GetUTF8StringBlobIndex(parts[i]));
            }

            return signature;
        }

        private static bool TryGetDocumentNameSeparator(string path, out char separator)
        {
            const char unix = '/';
            const char win = '\\';
            const char zero = (char)0;

            separator = zero;
            if (string.IsNullOrEmpty(path))
                return false;

            int unix_count = 0;
            int win_count = 0;

            for (int i = 0; i < path.Length; i++)
            {
                if (path[i] == unix)
                    unix_count++;
                else if (path[i] == win)
                    win_count++;
            }

            if (unix_count == 0 && win_count == 0)
                return false;

            if (unix_count >= win_count)
            {
                separator = unix;
                return true;
            }

            separator = win;
            return true;
        }

        private void AddSequencePoints(MethodDebugInformation info)
        {
            uint rid = info.Method.MetadataToken.RID;

            Document document;
            if (info.TryGetUniqueDocument(out document))
                method_debug_information_table.rows[rid - 1].Col1 = GetDocumentToken(document).RID;

            SignatureWriter signature = CreateSignatureWriter();
            signature.WriteSequencePoints(info);

            method_debug_information_table.rows[rid - 1].Col2 = GetBlobIndex(signature);
        }

        public void ComputeDeterministicMvid()
        {
            Guid guid = CryptoService.ComputeGuid(CryptoService.ComputeHash(data, resources, string_heap, user_string_heap, blob_heap, table_heap, code));

            int position = guid_heap.position;
            guid_heap.position = 0;
            guid_heap.WriteBytes(guid.ToByteArray());
            guid_heap.position = position;

            module.Mvid = guid;
        }
    }

    internal sealed class SignatureWriter : ByteBuffer
    {
        private readonly MetadataBuilder metadata;

        public SignatureWriter(MetadataBuilder metadata) : base(6)
        {
            this.metadata = metadata;
        }

        public void WriteElementType(ElementType element_type)
        {
            WriteByte((byte)element_type);
        }

        public void WriteUTF8String(string @string)
        {
            if (@string == null)
            {
                WriteByte(0xff);
                return;
            }

            byte[] bytes = Encoding.UTF8.GetBytes(@string);
            WriteCompressedUInt32((uint)bytes.Length);
            WriteBytes(bytes);
        }

        public void WriteMethodSignature(IMethodSignature method)
        {
            byte calling_convention = (byte)method.CallingConvention;
            if (method.HasThis)
                calling_convention |= 0x20;
            if (method.ExplicitThis)
                calling_convention |= 0x40;

            IGenericParameterProvider generic_provider = method as IGenericParameterProvider;
            int generic_arity = generic_provider != null && generic_provider.HasGenericParameters ? generic_provider.GenericParameters.Count : 0;

            if (generic_arity > 0)
                calling_convention |= 0x10;

            int param_count = method.HasParameters ? method.Parameters.Count : 0;

            WriteByte(calling_convention);

            if (generic_arity > 0)
                WriteCompressedUInt32((uint)generic_arity);

            WriteCompressedUInt32((uint)param_count);
            WriteTypeSignature(method.ReturnType);

            if (param_count == 0)
                return;

            Collection<ParameterDefinition> parameters = method.Parameters;

            for (int i = 0; i < param_count; i++)
                WriteTypeSignature(parameters[i].ParameterType);
        }

        private uint MakeTypeDefOrRefCodedRID(TypeReference type)
        {
            return CodedIndex.TypeDefOrRef.CompressMetadataToken(metadata.LookupToken(type));
        }

        public void WriteTypeToken(TypeReference type)
        {
            WriteCompressedUInt32(MakeTypeDefOrRefCodedRID(type));
        }

        public void WriteTypeSignature(TypeReference type)
        {
            if (type == null)
                throw new ArgumentNullException();

            ElementType etype = type.etype;

            switch (etype)
            {
                case ElementType.MVar:
                case ElementType.Var:
                {
                    GenericParameter generic_parameter = (GenericParameter)type;

                    WriteElementType(etype);
                    int position = generic_parameter.Position;
                    if (position == -1)
                        throw new NotSupportedException();

                    WriteCompressedUInt32((uint)position);
                    break;
                }

                case ElementType.GenericInst:
                {
                    GenericInstanceType generic_instance = (GenericInstanceType)type;
                    WriteElementType(ElementType.GenericInst);
                    WriteElementType(generic_instance.IsValueType ? ElementType.ValueType : ElementType.Class);
                    WriteCompressedUInt32(MakeTypeDefOrRefCodedRID(generic_instance.ElementType));

                    WriteGenericInstanceSignature(generic_instance);
                    break;
                }

                case ElementType.Ptr:
                case ElementType.ByRef:
                case ElementType.Pinned:
                case ElementType.Sentinel:
                {
                    TypeSpecification type_spec = (TypeSpecification)type;
                    WriteElementType(etype);
                    WriteTypeSignature(type_spec.ElementType);
                    break;
                }

                case ElementType.FnPtr:
                {
                    FunctionPointerType fptr = (FunctionPointerType)type;
                    WriteElementType(ElementType.FnPtr);
                    WriteMethodSignature(fptr);
                    break;
                }

                case ElementType.CModOpt:
                case ElementType.CModReqD:
                {
                    IModifierType modifier = (IModifierType)type;
                    WriteModifierSignature(etype, modifier);
                    break;
                }

                case ElementType.Array:
                {
                    ArrayType array = (ArrayType)type;
                    if (!array.IsVector)
                    {
                        WriteArrayTypeSignature(array);
                        break;
                    }

                    WriteElementType(ElementType.SzArray);
                    WriteTypeSignature(array.ElementType);
                    break;
                }

                case ElementType.None:
                {
                    WriteElementType(type.IsValueType ? ElementType.ValueType : ElementType.Class);
                    WriteCompressedUInt32(MakeTypeDefOrRefCodedRID(type));
                    break;
                }

                default:
                    if (!TryWriteElementType(type))
                        throw new NotSupportedException();

                    break;
            }
        }

        private void WriteArrayTypeSignature(ArrayType array)
        {
            WriteElementType(ElementType.Array);
            WriteTypeSignature(array.ElementType);

            Collection<ArrayDimension> dimensions = array.Dimensions;
            int rank = dimensions.Count;

            WriteCompressedUInt32((uint)rank);

            int sized = 0;
            int lbounds = 0;

            for (int i = 0; i < rank; i++)
            {
                ArrayDimension dimension = dimensions[i];

                if (dimension.UpperBound.HasValue)
                {
                    sized++;
                    lbounds++;
                }
                else if (dimension.LowerBound.HasValue)
                {
                    lbounds++;
                }
            }

            int[] sizes = new int [sized];
            int[] low_bounds = new int [lbounds];

            for (int i = 0; i < lbounds; i++)
            {
                ArrayDimension dimension = dimensions[i];
                low_bounds[i] = dimension.LowerBound.GetValueOrDefault();
                if (dimension.UpperBound.HasValue)
                    sizes[i] = dimension.UpperBound.Value - low_bounds[i] + 1;
            }

            WriteCompressedUInt32((uint)sized);
            for (int i = 0; i < sized; i++)
                WriteCompressedUInt32((uint)sizes[i]);

            WriteCompressedUInt32((uint)lbounds);
            for (int i = 0; i < lbounds; i++)
                WriteCompressedInt32(low_bounds[i]);
        }

        public void WriteGenericInstanceSignature(IGenericInstance instance)
        {
            Collection<TypeReference> generic_arguments = instance.GenericArguments;
            int arity = generic_arguments.Count;

            WriteCompressedUInt32((uint)arity);
            for (int i = 0; i < arity; i++)
                WriteTypeSignature(generic_arguments[i]);
        }

        private void WriteModifierSignature(ElementType element_type, IModifierType type)
        {
            WriteElementType(element_type);
            WriteCompressedUInt32(MakeTypeDefOrRefCodedRID(type.ModifierType));
            WriteTypeSignature(type.ElementType);
        }

        private bool TryWriteElementType(TypeReference type)
        {
            ElementType element = type.etype;

            if (element == ElementType.None)
                return false;

            WriteElementType(element);
            return true;
        }

        public void WriteConstantString(string value)
        {
            if (value != null)
                WriteBytes(Encoding.Unicode.GetBytes(value));
            else
                WriteByte(0xff);
        }

        public void WriteConstantPrimitive(object value)
        {
            WritePrimitiveValue(value);
        }

        public void WriteCustomAttributeConstructorArguments(CustomAttribute attribute)
        {
            if (!attribute.HasConstructorArguments)
                return;

            Collection<CustomAttributeArgument> arguments = attribute.ConstructorArguments;
            Collection<ParameterDefinition> parameters = attribute.Constructor.Parameters;

            if (parameters.Count != arguments.Count)
                throw new InvalidOperationException();

            for (int i = 0; i < arguments.Count; i++)
                WriteCustomAttributeFixedArgument(parameters[i].ParameterType, arguments[i]);
        }

        private void WriteCustomAttributeFixedArgument(TypeReference type, CustomAttributeArgument argument)
        {
            if (type.IsArray)
            {
                WriteCustomAttributeFixedArrayArgument((ArrayType)type, argument);
                return;
            }

            WriteCustomAttributeElement(type, argument);
        }

        private void WriteCustomAttributeFixedArrayArgument(ArrayType type, CustomAttributeArgument argument)
        {
            CustomAttributeArgument[] values = argument.Value as CustomAttributeArgument[];

            if (values == null)
            {
                WriteUInt32(0xffffffff);
                return;
            }

            WriteInt32(values.Length);

            if (values.Length == 0)
                return;

            TypeReference element_type = type.ElementType;

            for (int i = 0; i < values.Length; i++)
                WriteCustomAttributeElement(element_type, values[i]);
        }

        private void WriteCustomAttributeElement(TypeReference type, CustomAttributeArgument argument)
        {
            if (type.IsArray)
            {
                WriteCustomAttributeFixedArrayArgument((ArrayType)type, argument);
                return;
            }

            if (type.etype == ElementType.Object)
            {
                argument = (CustomAttributeArgument)argument.Value;
                type = argument.Type;

                WriteCustomAttributeFieldOrPropType(type);
                WriteCustomAttributeElement(type, argument);
                return;
            }

            WriteCustomAttributeValue(type, argument.Value);
        }

        private void WriteCustomAttributeValue(TypeReference type, object value)
        {
            ElementType etype = type.etype;

            switch (etype)
            {
                case ElementType.String:
                    string @string = (string)value;
                    if (@string == null)
                        WriteByte(0xff);
                    else
                        WriteUTF8String(@string);
                    break;
                case ElementType.None:
                    if (type.IsTypeOf("System", "Type"))
                        WriteCustomAttributeTypeValue((TypeReference)value);
                    else
                        WriteCustomAttributeEnumValue(type, value);
                    break;
                default:
                    WritePrimitiveValue(value);
                    break;
            }
        }

        private void WriteCustomAttributeTypeValue(TypeReference value)
        {
            TypeDefinition typeDefinition = value as TypeDefinition;

            if (typeDefinition != null)
            {
                TypeDefinition outermostDeclaringType = typeDefinition;
                while (outermostDeclaringType.DeclaringType != null)
                    outermostDeclaringType = outermostDeclaringType.DeclaringType;

                // In CLR .winmd files, custom attribute arguments reference unmangled type names (rather than <CLR>Name)
                if (WindowsRuntimeProjections.IsClrImplementationType(outermostDeclaringType))
                {
                    WindowsRuntimeProjections.Project(outermostDeclaringType);
                    WriteTypeReference(value);
                    WindowsRuntimeProjections.RemoveProjection(outermostDeclaringType);
                    return;
                }
            }

            WriteTypeReference(value);
        }

        private void WritePrimitiveValue(object value)
        {
            if (value == null)
                throw new ArgumentNullException();

            switch (Type.GetTypeCode(value.GetType()))
            {
                case TypeCode.Boolean:
                    WriteByte((byte)((bool)value ? 1 : 0));
                    break;
                case TypeCode.Byte:
                    WriteByte((byte)value);
                    break;
                case TypeCode.SByte:
                    WriteSByte((sbyte)value);
                    break;
                case TypeCode.Int16:
                    WriteInt16((short)value);
                    break;
                case TypeCode.UInt16:
                    WriteUInt16((ushort)value);
                    break;
                case TypeCode.Char:
                    WriteInt16((short)(char)value);
                    break;
                case TypeCode.Int32:
                    WriteInt32((int)value);
                    break;
                case TypeCode.UInt32:
                    WriteUInt32((uint)value);
                    break;
                case TypeCode.Single:
                    WriteSingle((float)value);
                    break;
                case TypeCode.Int64:
                    WriteInt64((long)value);
                    break;
                case TypeCode.UInt64:
                    WriteUInt64((ulong)value);
                    break;
                case TypeCode.Double:
                    WriteDouble((double)value);
                    break;
                default:
                    throw new NotSupportedException(value.GetType().FullName);
            }
        }

        private void WriteCustomAttributeEnumValue(TypeReference enum_type, object value)
        {
            TypeDefinition type = enum_type.CheckedResolve();
            if (!type.IsEnum)
                throw new ArgumentException();

            WriteCustomAttributeValue(type.GetEnumUnderlyingType(), value);
        }

        private void WriteCustomAttributeFieldOrPropType(TypeReference type)
        {
            if (type.IsArray)
            {
                ArrayType array = (ArrayType)type;
                WriteElementType(ElementType.SzArray);
                WriteCustomAttributeFieldOrPropType(array.ElementType);
                return;
            }

            ElementType etype = type.etype;

            switch (etype)
            {
                case ElementType.Object:
                    WriteElementType(ElementType.Boxed);
                    return;
                case ElementType.None:
                    if (type.IsTypeOf("System", "Type"))
                    {
                        WriteElementType(ElementType.Type);
                    }
                    else
                    {
                        WriteElementType(ElementType.Enum);
                        WriteTypeReference(type);
                    }
                    return;
                default:
                    WriteElementType(etype);
                    return;
            }
        }

        public void WriteCustomAttributeNamedArguments(CustomAttribute attribute)
        {
            int count = GetNamedArgumentCount(attribute);

            WriteUInt16((ushort)count);

            if (count == 0)
                return;

            WriteICustomAttributeNamedArguments(attribute);
        }

        private static int GetNamedArgumentCount(ICustomAttribute attribute)
        {
            int count = 0;

            if (attribute.HasFields)
                count += attribute.Fields.Count;

            if (attribute.HasProperties)
                count += attribute.Properties.Count;

            return count;
        }

        private void WriteICustomAttributeNamedArguments(ICustomAttribute attribute)
        {
            if (attribute.HasFields)
                WriteCustomAttributeNamedArguments(0x53, attribute.Fields);

            if (attribute.HasProperties)
                WriteCustomAttributeNamedArguments(0x54, attribute.Properties);
        }

        private void WriteCustomAttributeNamedArguments(byte kind, Collection<CustomAttributeNamedArgument> named_arguments)
        {
            for (int i = 0; i < named_arguments.Count; i++)
                WriteCustomAttributeNamedArgument(kind, named_arguments[i]);
        }

        private void WriteCustomAttributeNamedArgument(byte kind, CustomAttributeNamedArgument named_argument)
        {
            CustomAttributeArgument argument = named_argument.Argument;

            WriteByte(kind);
            WriteCustomAttributeFieldOrPropType(argument.Type);
            WriteUTF8String(named_argument.Name);
            WriteCustomAttributeFixedArgument(argument.Type, argument);
        }

        private void WriteSecurityAttribute(SecurityAttribute attribute)
        {
            WriteTypeReference(attribute.AttributeType);

            int count = GetNamedArgumentCount(attribute);

            if (count == 0)
            {
                WriteCompressedUInt32(1); // length
                WriteCompressedUInt32(0); // count
                return;
            }

            SignatureWriter buffer = new(metadata);
            buffer.WriteCompressedUInt32((uint)count);
            buffer.WriteICustomAttributeNamedArguments(attribute);

            WriteCompressedUInt32((uint)buffer.length);
            WriteBytes(buffer);
        }

        public void WriteSecurityDeclaration(SecurityDeclaration declaration)
        {
            WriteByte((byte)'.');

            Collection<SecurityAttribute> attributes = declaration.security_attributes;
            if (attributes == null)
                throw new NotSupportedException();

            WriteCompressedUInt32((uint)attributes.Count);

            for (int i = 0; i < attributes.Count; i++)
                WriteSecurityAttribute(attributes[i]);
        }

        public void WriteXmlSecurityDeclaration(SecurityDeclaration declaration)
        {
            string xml = GetXmlSecurityDeclaration(declaration);
            if (xml == null)
                throw new NotSupportedException();

            WriteBytes(Encoding.Unicode.GetBytes(xml));
        }

        private static string GetXmlSecurityDeclaration(SecurityDeclaration declaration)
        {
            if (declaration.security_attributes == null || declaration.security_attributes.Count != 1)
                return null;

            SecurityAttribute attribute = declaration.security_attributes[0];

            if (!attribute.AttributeType.IsTypeOf("System.Security.Permissions", "PermissionSetAttribute"))
                return null;

            if (attribute.properties == null || attribute.properties.Count != 1)
                return null;

            CustomAttributeNamedArgument property = attribute.properties[0];
            if (property.Name != "XML")
                return null;

            return (string)property.Argument.Value;
        }

        private void WriteTypeReference(TypeReference type)
        {
            WriteUTF8String(TypeParser.ToParseable(type, top_level: false));
        }

        public void WriteMarshalInfo(MarshalInfo marshal_info)
        {
            WriteNativeType(marshal_info.native);

            switch (marshal_info.native)
            {
                case NativeType.Array:
                {
                    ArrayMarshalInfo array = (ArrayMarshalInfo)marshal_info;
                    if (array.element_type != NativeType.None)
                        WriteNativeType(array.element_type);
                    if (array.size_parameter_index > -1)
                        WriteCompressedUInt32((uint)array.size_parameter_index);
                    if (array.size > -1)
                        WriteCompressedUInt32((uint)array.size);
                    if (array.size_parameter_multiplier > -1)
                        WriteCompressedUInt32((uint)array.size_parameter_multiplier);
                    return;
                }
                case NativeType.SafeArray:
                {
                    SafeArrayMarshalInfo array = (SafeArrayMarshalInfo)marshal_info;
                    if (array.element_type != VariantType.None)
                        WriteVariantType(array.element_type);
                    return;
                }
                case NativeType.FixedArray:
                {
                    FixedArrayMarshalInfo array = (FixedArrayMarshalInfo)marshal_info;
                    if (array.size > -1)
                        WriteCompressedUInt32((uint)array.size);
                    if (array.element_type != NativeType.None)
                        WriteNativeType(array.element_type);
                    return;
                }
                case NativeType.FixedSysString:
                    FixedSysStringMarshalInfo sys_string = (FixedSysStringMarshalInfo)marshal_info;
                    if (sys_string.size > -1)
                        WriteCompressedUInt32((uint)sys_string.size);
                    return;
                case NativeType.CustomMarshaler:
                    CustomMarshalInfo marshaler = (CustomMarshalInfo)marshal_info;
                    WriteUTF8String(marshaler.guid != Guid.Empty ? marshaler.guid.ToString() : string.Empty);
                    WriteUTF8String(marshaler.unmanaged_type);
                    WriteTypeReference(marshaler.managed_type);
                    WriteUTF8String(marshaler.cookie);
                    return;
            }
        }

        private void WriteNativeType(NativeType native)
        {
            WriteByte((byte)native);
        }

        private void WriteVariantType(VariantType variant)
        {
            WriteByte((byte)variant);
        }

        public void WriteSequencePoints(MethodDebugInformation info)
        {
            int start_line = -1;
            int start_column = -1;

            WriteCompressedUInt32(info.local_var_token.RID);

            Document previous_document;
            if (!info.TryGetUniqueDocument(out previous_document))
                previous_document = null;

            for (int i = 0; i < info.SequencePoints.Count; i++)
            {
                SequencePoint sequence_point = info.SequencePoints[i];

                Document document = sequence_point.Document;
                if (previous_document != document)
                {
                    MetadataToken document_token = metadata.GetDocumentToken(document);

                    if (previous_document != null)
                        WriteCompressedUInt32(0);

                    WriteCompressedUInt32(document_token.RID);
                    previous_document = document;
                }

                if (i > 0)
                    WriteCompressedUInt32((uint)(sequence_point.Offset - info.SequencePoints[i - 1].Offset));
                else
                    WriteCompressedUInt32((uint)sequence_point.Offset);

                if (sequence_point.IsHidden)
                {
                    WriteInt16(0);
                    continue;
                }

                int delta_lines = sequence_point.EndLine - sequence_point.StartLine;
                int delta_columns = sequence_point.EndColumn - sequence_point.StartColumn;

                WriteCompressedUInt32((uint)delta_lines);

                if (delta_lines == 0)
                    WriteCompressedUInt32((uint)delta_columns);
                else
                    WriteCompressedInt32(delta_columns);

                if (start_line < 0)
                {
                    WriteCompressedUInt32((uint)sequence_point.StartLine);
                    WriteCompressedUInt32((uint)sequence_point.StartColumn);
                }
                else
                {
                    WriteCompressedInt32(sequence_point.StartLine - start_line);
                    WriteCompressedInt32(sequence_point.StartColumn - start_column);
                }

                start_line = sequence_point.StartLine;
                start_column = sequence_point.StartColumn;
            }
        }
    }

    internal static partial class Mixin
    {
        public static bool TryGetUniqueDocument(this MethodDebugInformation info, out Document document)
        {
            document = info.SequencePoints[0].Document;

            for (int i = 1; i < info.SequencePoints.Count; i++)
            {
                SequencePoint sequence_point = info.SequencePoints[i];
                if (sequence_point.Document != document)
                    return false;
            }

            return true;
        }
    }
}