//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

namespace MonoFN.Cecil
{
    public sealed class AssemblyLinkedResource : Resource
    {
        public AssemblyNameReference Assembly { get; set; }
        public override ResourceType ResourceType
        {
            get { return ResourceType.AssemblyLinked; }
        }
        public AssemblyLinkedResource(string name, ManifestResourceAttributes flags) : base(name, flags) { }

        public AssemblyLinkedResource(string name, ManifestResourceAttributes flags, AssemblyNameReference reference) : base(name, flags)
        {
            this.Assembly = reference;
        }
    }
}