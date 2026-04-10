import React from 'react'

function NetworkBadge() {
    const network = process.env.NEXT_PUBLIC_NETWORK ?? 'devnet';
    return (
      <span className={`text-xs px-2 py-0.5 font-mono text-[#8F8D8E]`}>
        {network}
      </span>
    );
  }

export default NetworkBadge