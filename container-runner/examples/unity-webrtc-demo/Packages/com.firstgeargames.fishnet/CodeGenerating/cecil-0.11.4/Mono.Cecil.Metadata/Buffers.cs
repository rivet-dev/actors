//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

using MonoFN.Cecil.PE;
using System;
using System.Collections.Generic;
using System.Text;
using RVA = System.UInt32;

namespace MonoFN.Cecil.Metadata
{
    internal sealed class TableHeapBuffer : HeapBuffer
    {
        private readonly ModuleDefinition module;
        private readonly MetadataBuilder metadata;
        internal readonly TableInformation[] table_infos = new TableInformation [Mixin.TableCount];
        internal readonly MetadataTable[] tables = new MetadataTable [Mixin.TableCount];
        private bool large_string;
        private bool large_blob;
        private bool large_guid;
        private readonly int[] coded_index_sizes = new int [Mixin.CodedIndexCount];
        private readonly Func<Table, int> counter;
        internal uint[] string_offsets;
        public override bool IsEmpty
        {
            get { return false; }
        }

        public TableHeapBuffer(ModuleDefinition module, MetadataBuilder metadata) : base(24)
        {
            this.module = module;
            this.metadata = metadata;
            counter = GetTableLength;
        }

        private int GetTableLength(Table table)
        {
            return (int)table_infos[(int)table].Length;
        }

        public TTable GetTable<TTable>(Table table) where TTable : MetadataTable, new()
        {
            TTable md_table = (TTable)tables[(int)table];
            if (md_table != null)
                return md_table;

            md_table = new();
            tables[(int)table] = md_table;
            return md_table;
        }

        public void WriteBySize(uint value, int size)
        {
            if (size == 4)
                WriteUInt32(value);
            else
                WriteUInt16((ushort)value);
        }

        public void WriteBySize(uint value, bool large)
        {
            if (large)
                WriteUInt32(value);
            else
                WriteUInt16((ushort)value);
        }

        public void WriteString(uint @string)
        {
            WriteBySize(string_offsets[@string], large_string);
        }

        public void WriteBlob(uint blob)
        {
            WriteBySize(blob, large_blob);
        }

        public void WriteGuid(uint guid)
        {
            WriteBySize(guid, large_guid);
        }

        public void WriteRID(uint rid, Table table)
        {
            WriteBySize(rid, table_infos[(int)table].IsLarge);
        }

        private int GetCodedIndexSize(CodedIndex coded_index)
        {
            int index = (int)coded_index;
            int size = coded_index_sizes[index];
            if (size != 0)
                return size;

            return coded_index_sizes[index] = coded_index.GetSize(counter);
        }

        public void WriteCodedRID(uint rid, CodedIndex coded_index)
        {
            WriteBySize(rid, GetCodedIndexSize(coded_index));
        }

        public void WriteTableHeap()
        {
            WriteUInt32(0); // Reserved
            WriteByte(GetTableHeapVersion()); // MajorVersion
            WriteByte(0); // MinorVersion
            WriteByte(GetHeapSizes()); // HeapSizes
            WriteByte(10); // Reserved2
            WriteUInt64(GetValid()); // Valid
            WriteUInt64(0xc416003301fa00); // Sorted

            WriteRowCount();
            WriteTables();
        }

        private void WriteRowCount()
        {
            for (int i = 0; i < tables.Length; i++)
            {
                MetadataTable table = tables[i];
                if (table == null || table.Length == 0)
                    continue;

                WriteUInt32((uint)table.Length);
            }
        }

        private void WriteTables()
        {
            for (int i = 0; i < tables.Length; i++)
            {
                MetadataTable table = tables[i];
                if (table == null || table.Length == 0)
                    continue;

                table.Write(this);
            }
        }

        private ulong GetValid()
        {
            ulong valid = 0;

            for (int i = 0; i < tables.Length; i++)
            {
                MetadataTable table = tables[i];
                if (table == null || table.Length == 0)
                    continue;

                table.Sort();
                valid |= 1UL << i;
            }

            return valid;
        }

        public void ComputeTableInformations()
        {
            if (metadata.metadata_builder != null)
                ComputeTableInformations(metadata.metadata_builder.table_heap);

            ComputeTableInformations(metadata.table_heap);
        }

        private void ComputeTableInformations(TableHeapBuffer table_heap)
        {
            MetadataTable[] tables = table_heap.tables;
            for (int i = 0; i < tables.Length; i++)
            {
                MetadataTable table = tables[i];
                if (table != null && table.Length > 0)
                    table_infos[i].Length = (uint)table.Length;
            }
        }

        private byte GetHeapSizes()
        {
            byte heap_sizes = 0;

            if (metadata.string_heap.IsLarge)
            {
                large_string = true;
                heap_sizes |= 0x01;
            }

            if (metadata.guid_heap.IsLarge)
            {
                large_guid = true;
                heap_sizes |= 0x02;
            }

            if (metadata.blob_heap.IsLarge)
            {
                large_blob = true;
                heap_sizes |= 0x04;
            }

            return heap_sizes;
        }

        private byte GetTableHeapVersion()
        {
            switch (module.Runtime)
            {
                case TargetRuntime.Net_1_0:
                case TargetRuntime.Net_1_1:
                    return 1;
                default:
                    return 2;
            }
        }

        public void FixupData(RVA data_rva)
        {
            FieldRVATable table = GetTable<FieldRVATable>(Table.FieldRVA);
            if (table.length == 0)
                return;

            int field_idx_size = GetTable<FieldTable>(Table.Field).IsLarge ? 4 : 2;
            int previous = position;

            position = table.position;
            for (int i = 0; i < table.length; i++)
            {
                uint rva = ReadUInt32();
                position -= 4;
                WriteUInt32(rva + data_rva);
                position += field_idx_size;
            }

            position = previous;
        }
    }

    internal sealed class ResourceBuffer : ByteBuffer
    {
        public ResourceBuffer() : base(0) { }

        public uint AddResource(byte[] resource)
        {
            uint offset = (uint)position;
            WriteInt32(resource.Length);
            WriteBytes(resource);
            return offset;
        }
    }

    internal sealed class DataBuffer : ByteBuffer
    {
        public DataBuffer() : base(0) { }

        public RVA AddData(byte[] data)
        {
            RVA rva = (RVA)position;
            WriteBytes(data);
            return rva;
        }
    }

    internal abstract class HeapBuffer : ByteBuffer
    {
        public bool IsLarge
        {
            get { return length > 65535; }
        }
        public abstract bool IsEmpty { get; }
        protected HeapBuffer(int length) : base(length) { }
    }

    internal sealed class GuidHeapBuffer : HeapBuffer
    {
        private readonly Dictionary<Guid, uint> guids = new();
        public override bool IsEmpty
        {
            get { return length == 0; }
        }
        public GuidHeapBuffer() : base(16) { }

        public uint GetGuidIndex(Guid guid)
        {
            uint index;
            if (guids.TryGetValue(guid, out index))
                return index;

            index = (uint)guids.Count + 1;
            WriteGuid(guid);
            guids.Add(guid, index);
            return index;
        }

        private void WriteGuid(Guid guid)
        {
            WriteBytes(guid.ToByteArray());
        }
    }

    internal class StringHeapBuffer : HeapBuffer
    {
        protected Dictionary<string, uint> strings = new(StringComparer.Ordinal);
        public sealed override bool IsEmpty
        {
            get { return length <= 1; }
        }

        public StringHeapBuffer() : base(1)
        {
            WriteByte(0);
        }

        public virtual uint GetStringIndex(string @string)
        {
            uint index;
            if (strings.TryGetValue(@string, out index))
                return index;

            index = (uint)strings.Count + 1;
            strings.Add(@string, index);
            return index;
        }

        public uint[] WriteStrings()
        {
            List<KeyValuePair<string, uint>> sorted = SortStrings(strings);
            strings = null;

            // Add 1 for empty string whose index and offset are both 0
            uint[] string_offsets = new uint [sorted.Count + 1];
            string_offsets[0] = 0;

            // Find strings that can be folded
            string previous = string.Empty;
            foreach (KeyValuePair<string, uint> entry in sorted)
            {
                string @string = entry.Key;
                uint index = entry.Value;
                int position = this.position;

                if (previous.EndsWith(@string, StringComparison.Ordinal) && !IsLowSurrogateChar(entry.Key[0]))
                {
                    // Map over the tail of prev string. Watch for null-terminator of prev string.
                    string_offsets[index] = (uint)(position - (Encoding.UTF8.GetByteCount(entry.Key) + 1));
                }
                else
                {
                    string_offsets[index] = (uint)position;
                    WriteString(@string);
                }

                previous = entry.Key;
            }

            return string_offsets;
        }

        private static List<KeyValuePair<string, uint>> SortStrings(Dictionary<string, uint> strings)
        {
            List<KeyValuePair<string, uint>> sorted = new(strings);
            sorted.Sort(new SuffixSort());
            return sorted;
        }

        private static bool IsLowSurrogateChar(int c)
        {
            return unchecked((uint)(c - 0xDC00)) <= 0xDFFF - 0xDC00;
        }

        protected virtual void WriteString(string @string)
        {
            WriteBytes(Encoding.UTF8.GetBytes(@string));
            WriteByte(0);
        }

        // Sorts strings such that a string is followed immediately by all strings
        // that are a suffix of it.  
        private class SuffixSort : IComparer<KeyValuePair<string, uint>>
        {
            public int Compare(KeyValuePair<string, uint> xPair, KeyValuePair<string, uint> yPair)
            {
                string x = xPair.Key;
                string y = yPair.Key;

                for (int i = x.Length - 1, j = y.Length - 1; (i >= 0) & (j >= 0); i--, j--)
                {
                    if (x[i] < y[j])
                    {
                        return -1;
                    }

                    if (x[i] > y[j])
                    {
                        return +1;
                    }
                }

                return y.Length.CompareTo(x.Length);
            }
        }
    }

    internal sealed class BlobHeapBuffer : HeapBuffer
    {
        private readonly Dictionary<ByteBuffer, uint> blobs = new(new ByteBufferEqualityComparer());
        public override bool IsEmpty
        {
            get { return length <= 1; }
        }

        public BlobHeapBuffer() : base(1)
        {
            WriteByte(0);
        }

        public uint GetBlobIndex(ByteBuffer blob)
        {
            uint index;
            if (blobs.TryGetValue(blob, out index))
                return index;

            index = (uint)position;
            WriteBlob(blob);
            blobs.Add(blob, index);
            return index;
        }

        private void WriteBlob(ByteBuffer blob)
        {
            WriteCompressedUInt32((uint)blob.length);
            WriteBytes(blob);
        }
    }

    internal sealed class UserStringHeapBuffer : StringHeapBuffer
    {
        public override uint GetStringIndex(string @string)
        {
            uint index;
            if (strings.TryGetValue(@string, out index))
                return index;

            index = (uint)position;
            WriteString(@string);
            strings.Add(@string, index);
            return index;
        }

        protected override void WriteString(string @string)
        {
            WriteCompressedUInt32((uint)@string.Length * 2 + 1);

            byte special = 0;

            for (int i = 0; i < @string.Length; i++)
            {
                char @char = @string[i];
                WriteUInt16(@char);

                if (special == 1)
                    continue;

                if (@char < 0x20 || @char > 0x7e)
                {
                    if (@char > 0x7e || (@char >= 0x01 && @char <= 0x08) || (@char >= 0x0e && @char <= 0x1f) || @char == 0x27 || @char == 0x2d)
                    {
                        special = 1;
                    }
                }
            }

            WriteByte(special);
        }
    }

    internal sealed class PdbHeapBuffer : HeapBuffer
    {
        public override bool IsEmpty
        {
            get { return false; }
        }
        public PdbHeapBuffer() : base(0) { }
    }
}