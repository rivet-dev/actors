//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

using System;
using MD = MonoFN.Cecil.Metadata;

namespace MonoFN.Cecil
{
    public sealed class PinnedType : TypeSpecification
    {
        public override bool IsValueType
        {
            get { return false; }
            set { throw new InvalidOperationException(); }
        }
        public override bool IsPinned
        {
            get { return true; }
        }

        public PinnedType(TypeReference type) : base(type)
        {
            Mixin.CheckType(type);
            etype = MD.ElementType.Pinned;
        }
    }
}