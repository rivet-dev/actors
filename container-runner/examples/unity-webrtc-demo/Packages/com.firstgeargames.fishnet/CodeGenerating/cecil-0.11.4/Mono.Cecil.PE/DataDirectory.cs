//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

using RVA = System.UInt32;

namespace MonoFN.Cecil.PE
{
    internal struct DataDirectory
    {
        public readonly RVA VirtualAddress;
        public readonly uint Size;
        public bool IsZero
        {
            get { return VirtualAddress == 0 && Size == 0; }
        }

        public DataDirectory(RVA rva, uint size)
        {
            VirtualAddress = rva;
            Size = size;
        }
    }
}