//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

namespace MonoFN.Cecil.Metadata
{
    internal abstract class Heap
    {
        public int IndexSize;
        internal readonly byte[] data;

        protected Heap(byte[] data)
        {
            this.data = data;
        }
    }
}